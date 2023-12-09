use clap::Parser;
use entity::{archive_rule, request, task, user};
use migration::MigratorTrait;
use sea_orm::{
    prelude::Uuid, sea_query::OnConflict, ActiveModelTrait, ActiveValue::Set, ColumnTrait,
    Database, DatabaseConnection, DbErr, EntityTrait, ModelTrait, QueryFilter, QueryOrder,
};
use serenity::{
    builder::{CreateComponents, CreateEmbed, CreateInteractionResponse, CreateMessage},
    model::{
        application::interaction::message_component::MessageComponentInteraction,
        id::ChannelId,
        prelude::{
            interaction::{application_command::ApplicationCommandInteraction, Interaction},
            UserId,
        },
    },
    prelude::{EventHandler, GatewayIntents},
};
use slashery::{SlashArgs, SlashCmd, SlashCmdType, SlashCmds};
use snafu::ResultExt;
use time::OffsetDateTime;

mod utils;

#[derive(clap::Parser)]
struct Opts {
    #[clap(long, env)]
    discord_token: String,
    #[clap(long, env)]
    discord_app_id: u64,
    #[clap(long, env)]
    database_url: String,
}

#[derive(SlashCmd)]
#[slashery(name = "request", kind = "SlashCmdType::ChatInput")]
/// Make a new request
struct MakeRequest {
    /// A summary of the request
    title: String,
    /// One or more tasks to be completed, separated by `;`
    tasks: String,
}

#[derive(SlashCmds)]
enum Cmd {
    MakeRequest(MakeRequest),
}

struct Handler {
    db: DatabaseConnection,
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn interaction_create(
        &self,
        ctx: serenity::prelude::Context,
        interaction: serenity::model::prelude::interaction::Interaction,
    ) {
        match interaction {
            Interaction::ApplicationCommand(cmd) => match Cmd::from_interaction(&cmd).unwrap() {
                Cmd::MakeRequest(req) => self.make_request(cmd, req, ctx).await,
            },
            Interaction::MessageComponent(comp) => match &*comp.data.custom_id {
                "claim-task" => self.claim_request_task(comp, ctx).await,
                "complete-task" => self.complete_request_task(comp, ctx).await,
                "repeat-request" => self.repeat_request(comp, ctx).await,
                id => panic!("unknown message component id {id:?}"),
            },
            _ => (),
        }
    }
}

impl Handler {
    async fn make_request(
        &self,
        cmd: ApplicationCommandInteraction,
        req: MakeRequest,
        ctx: serenity::prelude::Context,
    ) {
        let tasks = req.tasks.split(';').filter(|task| !task.is_empty());
        let user = get_user_by_discord(&self.db, cmd.user.id).await.unwrap();
        let request = request::ActiveModel {
            title: Set(req.title),
            created_by: Set(user.id),
            discord_channel_id: Set(Some(cmd.channel_id.0 as i64)),
            // We only know the message ID once it has been created, so defer until after
            // discord_message_id: Set(cmd.id.0 as i64),
            ..Default::default()
        }
        .insert(&self.db)
        .await
        .unwrap();
        task::Entity::insert_many(tasks.enumerate().map(|(i, task)| task::ActiveModel {
            request: Set(request.id),
            weight: Set(i as i32 + 1),
            task: Set(task.to_string()),
            ..Default::default()
        }))
        .exec(&self.db)
        .await
        .unwrap();

        let rendered = render_request(&self.db, request.id).await;
        cmd.create_interaction_response(&ctx.http, |r| rendered.create_interaction_response(r))
            .await
            .unwrap();

        let response_message = cmd.get_interaction_response(&ctx.http).await.unwrap();
        request::ActiveModel {
            discord_message_id: Set(Some(response_message.id.0 as i64)),
            ..request.into()
        }
        .update(&self.db)
        .await
        .unwrap();
    }

    async fn claim_request_task(
        &self,
        comp: MessageComponentInteraction,
        ctx: serenity::prelude::Context,
    ) {
        let user = get_user_by_discord(&self.db, comp.user.id).await.unwrap();
        let tasks = task::Entity::update_many()
            .set(task::ActiveModel {
                assigned_to: Set(Some(user.id)),
                started_at: Set(Some(OffsetDateTime::now_utc())),
                ..Default::default()
            })
            .filter(
                task::Column::Id
                    .is_in(comp.data.values.iter().map(|v| Uuid::parse_str(v).unwrap())),
            )
            .exec_with_returning(&self.db)
            .await
            .unwrap();
        let request_id = tasks.get(0).expect("no updated task").request;

        let rendered = render_request(&self.db, request_id).await;
        comp.edit_original_message(&ctx.http, |r| rendered.create_interaction_response(r))
            .await
            .unwrap();
    }

    async fn complete_request_task(
        &self,
        comp: MessageComponentInteraction,
        ctx: serenity::prelude::Context,
    ) {
        let user = get_user_by_discord(&self.db, comp.user.id).await.unwrap();
        let tasks = task::Entity::update_many()
            .set(task::ActiveModel {
                assigned_to: Set(Some(user.id)),
                completed_at: Set(Some(OffsetDateTime::now_utc())),
                ..Default::default()
            })
            .filter(
                task::Column::Id
                    .is_in(comp.data.values.iter().map(|v| Uuid::parse_str(v).unwrap())),
            )
            .exec_with_returning(&self.db)
            .await
            .unwrap();
        let request_id = tasks.get(0).expect("no updated task").request;

        let tasks = task::Entity::find()
            .filter(task::Column::Request.eq(request_id))
            .all(&self.db)
            .await
            .unwrap();

        let rendered = render_request(&self.db, request_id).await;
        // try to archive if required
        if tasks.iter().all(|t| t.completed_at.is_some()) {
            if let Some(archive_rule) = archive_rule::Entity::find_by_id(comp.channel_id.0 as i64)
                .one(&self.db)
                .await
                .unwrap()
            {
                let archive_channel = ctx
                    .cache
                    .guild_channel(ChannelId(archive_rule.to_channel as u64))
                    .expect("archive channel not found");
                let archived_msg = archive_channel
                    .send_message(&ctx, |msg| rendered.create_message(msg))
                    .await
                    .unwrap();
                comp.create_interaction_response(&ctx.http, |msg| {
                    msg.interaction_response_data(|r| {
                        r.ephemeral(true).content(format!(
                            "Request has been archived, see {}",
                            archived_msg.link()
                        ))
                    })
                })
                .await
                .unwrap();
                // apparently the interaction message counts as a followup, which should avoid
                // requiring permission to see the channel
                comp.delete_followup_message(&ctx.http, comp.message.id)
                    .await
                    .unwrap();
                request::ActiveModel {
                    id: sea_orm::ActiveValue::Unchanged(request_id),
                    discord_message_id: Set(Some(archived_msg.id.0 as i64)),
                    ..Default::default()
                }
                .update(&self.db)
                .await
                .unwrap();
                return;
            }
        }

        comp.edit_original_message(&ctx.http, |r| rendered.create_interaction_response(r))
            .await
            .unwrap();
    }

    async fn repeat_request(
        &self,
        comp: MessageComponentInteraction,
        ctx: serenity::prelude::Context,
    ) {
        let user = get_user_by_discord(&self.db, comp.user.id).await.unwrap();
        let original_request = request::Entity::find()
            .filter(request::Column::DiscordMessageId.eq(comp.message.id.0 as i64))
            .one(&self.db)
            .await
            .unwrap()
            .expect("original request not found");
        let original_tasks = original_request
            .find_related(task::Entity)
            .all(&self.db)
            .await
            .unwrap();
        let channel = ctx
            .cache
            .guild_channel(
                original_request
                    .discord_channel_id
                    .expect("no channel stored for original message") as u64,
            )
            .expect("channel of original message not found");
        let request = request::ActiveModel {
            title: Set(original_request.title),
            created_by: Set(user.id),
            discord_channel_id: Set(Some(channel.id.0 as i64)),
            ..Default::default()
        }
        .insert(&self.db)
        .await
        .unwrap();
        task::Entity::insert_many(original_tasks.into_iter().map(|task| task::ActiveModel {
            request: Set(request.id),
            weight: Set(task.weight),
            task: Set(task.task),
            ..Default::default()
        }))
        .exec(&self.db)
        .await
        .unwrap();

        let rendered = render_request(&self.db, request.id).await;
        let message = channel
            .send_message(&ctx.http, |msg| rendered.create_message(msg))
            .await
            .unwrap();
        comp.create_interaction_response(&ctx.http, |msg| {
            msg.interaction_response_data(|r| {
                r.ephemeral(true)
                    .content(format!("Request has been repeated, see {}", message.link()))
            })
        })
        .await
        .unwrap();

        request::ActiveModel {
            discord_message_id: Set(Some(message.id.0 as i64)),
            ..request.into()
        }
        .update(&self.db)
        .await
        .unwrap();
    }
}

#[snafu::report]
#[tokio::main]
async fn main() -> Result<(), snafu::Whatever> {
    let opts = Opts::parse();
    let db = Database::connect(opts.database_url)
        .await
        .whatever_context("failed to connect to database")?;
    migration::Migrator::up(&db, None)
        .await
        .whatever_context("failed to apply migrations")?;
    let mut discord = serenity::Client::builder(&opts.discord_token, GatewayIntents::GUILDS)
        .application_id(opts.discord_app_id)
        .event_handler(Handler { db })
        .await
        .whatever_context("failed to build discord client")?;
    discord
        .cache_and_http
        .http
        .create_global_application_commands(
            &serde_json::to_value(Cmd::meta())
                .whatever_context("failed to serialize discord commands")?,
        )
        .await
        .whatever_context("failed to create discord commands")?;
    discord
        .start()
        .await
        .whatever_context("failed to run discord bot")?;
    Ok(())
}

async fn get_user_by_discord(
    db: &DatabaseConnection,
    discord_user: UserId,
) -> Result<entity::user::Model, DbErr> {
    entity::prelude::User::insert(entity::user::ActiveModel {
        discord_user_id: Set(discord_user.0 as i64),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(entity::user::Column::DiscordUserId)
            // No-op update clause means the user is still returned by the upsert RETURNING
            .update_column(entity::user::Column::DiscordUserId)
            .to_owned(),
    )
    .exec_with_returning(db)
    .await
}

async fn render_request(db: &DatabaseConnection, request_id: Uuid) -> RenderedRequest {
    use std::fmt::Write;

    let request = request::Entity::find_by_id(request_id)
        .one(db)
        .await
        .unwrap()
        .expect("could not find request model");
    let task_created_by = request
        .find_related(user::Entity)
        .one(db)
        .await
        .unwrap()
        .expect("could not find creator of request");
    let tasks = request
        .find_related(task::Entity)
        .order_by_asc(task::Column::Weight)
        .find_with_related(user::Entity)
        .all(db)
        .await
        .unwrap();

    RenderedRequest {
        content: (format!(
            "Request by <@{}>: {}",
            task_created_by.discord_user_id, request.title
        )),
        embed: {
            let mut embed = CreateEmbed::default();
            embed.title("Tasks").description(
                tasks
                    .iter()
                    .map(|(task, task_users)| {
                        let mut task_str = format!(
                            "{}. {disabled}{}{disabled}",
                            task.weight,
                            &task.task,
                            disabled = task.completed_at.map_or("", |_| "~~")
                        );
                        let state = Some("completed")
                            .zip(task.completed_at)
                            .or(Some("claimed").zip(task.started_at));
                        if let Some((state, timestamp)) = state {
                            task_str
                                .write_fmt(format_args!(
                                    ", {state} at <t:{timestamp}> (<t:{timestamp}:R>)",
                                    timestamp = timestamp.unix_timestamp()
                                ))
                                .unwrap();
                            if let Some(assignee) = task
                                .assigned_to
                                .and_then(|id| task_users.iter().find(|u| u.id == id))
                            {
                                task_str
                                    .write_fmt(format_args!(" by <@{}>", assignee.discord_user_id))
                                    .unwrap();
                            }
                        }
                        task_str.push('\n');
                        task_str
                    })
                    .collect::<String>(),
            );
            embed
        },
        components: {
            let mut components = CreateComponents::default();
            let uncompleted_tasks = tasks
                .iter()
                .filter(|(task, _)| task.completed_at.is_none())
                .collect::<Vec<_>>();
            let unclaimed_tasks = uncompleted_tasks
                .iter()
                .filter(|(task, _)| task.started_at.is_none())
                .collect::<Vec<_>>();
            if !unclaimed_tasks.is_empty() {
                components.create_action_row(|row| {
                    row.create_select_menu(|menu| {
                        menu.custom_id("claim-task")
                            .placeholder("Claim task")
                            .options(|opts| {
                                unclaimed_tasks.iter().for_each(|(task, _)| {
                                    opts.create_option(|opt| {
                                        opt.value(task.id)
                                            .label(format!("{}. {}", task.weight, task.task))
                                    });
                                });
                                opts
                            })
                    })
                });
            }
            if !uncompleted_tasks.is_empty() {
                components.create_action_row(|row| {
                    row.create_select_menu(|menu| {
                        menu.custom_id("complete-task")
                            .placeholder("Mark task as completed")
                            .options(|opts| {
                                uncompleted_tasks.iter().for_each(|(task, _)| {
                                    opts.create_option(|opt| {
                                        opt.value(task.id)
                                            .label(format!("{}. {}", task.weight, task.task))
                                    });
                                });
                                opts
                            })
                    })
                });
            }
            if uncompleted_tasks.is_empty() && request.discord_channel_id.is_some() {
                components.create_action_row(|row| {
                    row.create_button(|button| button.custom_id("repeat-request").label("Repeat"))
                });
            }
            components
        },
    }
}

struct RenderedRequest {
    content: String,
    embed: CreateEmbed,
    components: CreateComponents,
}

impl RenderedRequest {
    fn create_interaction_response<'a, 'b>(
        self,
        r: &'a mut CreateInteractionResponse<'b>,
    ) -> &'a mut CreateInteractionResponse<'b> {
        r.interaction_response_data(|d| {
            d.content(self.content)
                .add_embed(self.embed)
                .set_components(self.components)
        })
    }

    fn create_message<'a, 'b>(self, r: &'a mut CreateMessage<'b>) -> &'a mut CreateMessage<'b> {
        r.content(self.content)
            .set_embed(self.embed)
            .set_components(self.components)
    }
}
