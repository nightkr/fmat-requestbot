use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{BuildHasher, BuildHasherDefault},
    str::FromStr,
};

use clap::Parser;
use entity::{archive_rule, request, task, user};
use migration::MigratorTrait;
use regex::Regex;
use sea_orm::{
    prelude::Uuid,
    sea_query::OnConflict,
    ActiveModelTrait,
    ActiveValue::{NotSet, Set},
    ColumnTrait, Database, DatabaseConnection, DbErr, EntityTrait, ModelTrait, QueryFilter,
    QueryOrder,
};
use serde::{de::IntoDeserializer, Deserialize};
use serenity::{
    builder::{
        CreateComponents, CreateEmbed, CreateInteractionResponse, CreateMessage,
        EditInteractionResponse,
    },
    model::{
        application::{
            command::CommandOptionChoice,
            interaction::message_component::MessageComponentInteraction,
        },
        id::ChannelId,
        prelude::{
            interaction::{application_command::ApplicationCommandInteraction, Interaction},
            UserId,
        },
    },
    prelude::{EventHandler, GatewayIntents},
};
use slashery::{SlashArg, SlashArgs, SlashCmd, SlashCmdType, SlashCmds};
use snafu::ResultExt;
use strum::IntoEnumIterator;
use time::OffsetDateTime;

mod utils;

const QUIPS: &[&str] = &[
    "Remember: There is no shadow council",
    "Have you driven over Nautilus today?",
    "quini bozo",
    "Powered by your hopes and dreams... delicious!",
    "9 out of 10 doctors recommend a daily diet of at least 10 rmats",
    "Break war BTW",
    "Almost as good as the old request bot",
    "Instructions unclear? Try reading them bottom-up!",
    "T2 will tech in 15 minutes",
    "Not sponsored by cryptocurrency gambling",
    "Abandoned Ward has been lost to the colonials",
    "F",
    "This command has failed successfully",
    "Kingstone is under attack",
    "Nuke Jade Cove",
    "QRF Deez Nutz",
    "You got any Delvins?",
    "SCOPE CREEP",
    "Daily reminder to press W",
    "Daily reminder to set your MPF queues",
];

#[derive(clap::Parser)]
struct Opts {
    #[clap(long, env)]
    discord_token: String,
    #[clap(long, env)]
    discord_app_id: u64,
    #[clap(long, env)]
    database_url: String,
}

#[derive(strum::AsRefStr, strum::EnumIter, strum::EnumString)]
enum RequestType {
    General,
    Truck,
    Flatbed,
    Freighter,
    Train,
}

impl RequestType {
    fn thumbnail(&self) -> Option<&'static str> {
        match self {
            RequestType::General => None,
            RequestType::Truck => Some("https://cdn.discordapp.com/attachments/919852056091701299/920553851008987196/Dunne_Transport_Vehicle_Icon.png"),
            RequestType::Flatbed => Some("https://cdn.discordapp.com/attachments/919852056091701299/920553850354688061/FlatbedTruckVehicleIcon.png"),
            RequestType::Freighter => Some("https://cdn.discordapp.com/attachments/1170732453116248226/1182871827995963444/image.png"),
            RequestType::Train => Some("https://cdn.discordapp.com/attachments/919852056091701299/1094794004945698938/ezgif.com-webp-to-png.png"),
        }
    }
}

impl SlashArg for RequestType {
    fn arg_parse(
        arg: Option<&serenity::model::prelude::application_command::CommandDataOption>,
    ) -> Result<Self, slashery::ArgFromInteractionError> {
        let arg = String::arg_parse(arg)?;
        RequestType::from_str(&arg).map_err(|_| {
            slashery::ArgFromInteractionError::InvalidValueForType {
                expected: serenity::model::application::command::CommandOptionType::String,
                got: arg.into(),
            }
        })
    }

    fn arg_discord_type() -> serenity::model::prelude::command::CommandOptionType {
        serenity::model::application::command::CommandOptionType::String
    }

    fn arg_required() -> bool {
        true
    }

    fn arg_choices() -> Vec<serenity::model::prelude::command::CommandOptionChoice> {
        Self::iter()
            .map(|ty| {
                // CommandOptionChoice doesn't have a default constructor, so we have to go this roundabout way to construct one...
                CommandOptionChoice::deserialize(<HashMap<_, _> as IntoDeserializer<
                    serde::de::value::Error,
                >>::into_deserializer(
                    HashMap::from([("name", ty.as_ref()), ("value", ty.as_ref())]),
                ))
                .unwrap()
            })
            .collect()
    }
}

#[derive(SlashCmd)]
#[slashery(name = "request", kind = "SlashCmdType::ChatInput")]
/// Make a new request
struct MakeRequest {
    /// A summary of the request
    title: String,
    /// One or more tasks to be completed, separated by `;`
    tasks: String,
    /// The kind of request
    kind: RequestType,
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
                "unclaim-task" => {
                    self.update_request_task_status(comp, ctx, TaskState::Unclaimed)
                        .await
                }
                "claim-task" => {
                    self.update_request_task_status(comp, ctx, TaskState::Claimed)
                        .await
                }
                "complete-task" => {
                    self.update_request_task_status(comp, ctx, TaskState::Completed)
                        .await
                }
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
        let multiply_regex = Regex::new(r"(?:\{(\d+)x\}|())(.*)").unwrap();
        let tasks = req
            .tasks
            .split(';')
            .filter(|task| !task.is_empty())
            .flat_map(|task| {
                let (_, [multiplier, task]) = multiply_regex
                    .captures(task.trim())
                    .expect("task did not match regex")
                    .extract();
                let multiplier = Some(multiplier)
                    .filter(|x| !str::is_empty(x))
                    .map_or(1, |x| x.parse::<usize>().unwrap());
                std::iter::repeat(task.trim()).take(multiplier)
            });
        let user = get_user_by_discord(&self.db, cmd.user.id).await.unwrap();
        let request = request::ActiveModel {
            title: Set(req.title),
            created_by: Set(user.id),
            discord_channel_id: Set(Some(cmd.channel_id.0 as i64)),
            thumbnail_url: Set(req.kind.thumbnail().map(str::to_string)),
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
        cmd.create_interaction_response(&ctx.http, |r| {
            rendered.clone().create_interaction_response(r)
        })
        .await
        .unwrap();

        // For some reason embed thumbnails are sometimes stripped out by Discord
        // Editing the message _seems_ to add it back in...
        cmd.edit_original_interaction_response(&ctx.http, |r| {
            rendered.edit_interaction_response(r)
        })
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

    async fn update_request_task_status(
        &self,
        comp: MessageComponentInteraction,
        ctx: serenity::prelude::Context,
        state: TaskState,
    ) {
        let user = get_user_by_discord(&self.db, comp.user.id).await.unwrap();
        let updated_tasks = task::Entity::update_many()
            .set(task::ActiveModel {
                assigned_to: Set(Some(user.id)),
                started_at: match &state {
                    TaskState::Unclaimed => Set(None),
                    TaskState::Claimed => Set(Some(OffsetDateTime::now_utc())),
                    TaskState::Completed => NotSet,
                },
                completed_at: match &state {
                    TaskState::Unclaimed | TaskState::Claimed => Set(None),
                    TaskState::Completed => Set(Some(OffsetDateTime::now_utc())),
                },
                ..Default::default()
            })
            .filter(
                task::Column::Id
                    .is_in(comp.data.values.iter().map(|v| Uuid::parse_str(v).unwrap())),
            )
            .exec_with_returning(&self.db)
            .await
            .unwrap();
        let request_id = updated_tasks.get(0).expect("no updated task").request;

        // try to archive if required
        let rendered = render_request(&self.db, request_id).await;
        if let Some(archive_channel) =
            should_archive_request_to(&self.db, request_id, comp.channel_id).await
        {
            let archive_channel = ctx
                .cache
                .guild_channel(archive_channel)
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
            thumbnail_url: Set(original_request.thumbnail_url),
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

async fn should_archive_request_to(
    db: &DatabaseConnection,
    request_id: Uuid,
    from_channel: ChannelId,
) -> Option<ChannelId> {
    let tasks = task::Entity::find()
        .filter(task::Column::Request.eq(request_id))
        .all(db)
        .await
        .unwrap();
    if tasks.iter().all(|t| t.completed_at.is_some()) {
        archive_rule::Entity::find_by_id(from_channel.0 as i64)
            .one(db)
            .await
            .unwrap()
            .map(|rule| ChannelId(rule.to_channel as u64))
    } else {
        None
    }
}

#[derive(PartialEq, Eq)]
enum TaskState {
    Unclaimed,
    Claimed,
    Completed,
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

    let quip = {
        let hash = BuildHasherDefault::<DefaultHasher>::default().hash_one(request_id);
        QUIPS[hash as usize % QUIPS.len()]
    };

    RenderedRequest {
        content: format!("# {}", request.title),
        embed: {
            let mut embed = CreateEmbed::default();
            embed.title("Tasks").footer(|f| f.text(quip)).description(
                tasks
                    .iter()
                    .flat_map(|(task, task_users)| {
                        let state = Some("completed")
                            .zip(task.completed_at)
                            .or(Some("claimed").zip(task.started_at));
                        let assignee = task
                            .assigned_to
                            .and_then(|id| task_users.iter().find(|u| u.id == id));
                        [
                            Some(format!(
                                "{}. {disabled}{}{disabled}",
                                task.weight,
                                &task.task,
                                disabled = task.completed_at.map_or("", |_| "~~")
                            )),
                            state.map(|(state, timestamp)| {
                                format!(
                                    ", {state} at <t:{timestamp}> (<t:{timestamp}:R>)",
                                    timestamp = timestamp.unix_timestamp()
                                )
                            }),
                            state
                                .and(assignee)
                                .map(|assignee| format!(" by <@{}>", assignee.discord_user_id)),
                            Some("\n".to_string()),
                        ]
                    })
                    .flatten()
                    .chain([format!(
                        "*Requested by <@{}>*",
                        task_created_by.discord_user_id
                    )])
                    .collect::<String>(),
            );
            if let Some(thumbnail_url) = &request.thumbnail_url {
                embed.thumbnail(thumbnail_url);
            }
            embed
        },
        components: {
            let mut components = CreateComponents::default();
            let uncompleted_tasks = tasks
                .iter()
                .filter(|(task, _)| task.completed_at.is_none())
                .collect::<Vec<_>>();
            let (claimed_tasks, unclaimed_tasks) = uncompleted_tasks
                .iter()
                .copied()
                .partition::<Vec<_>, _>(|(task, _)| task.started_at.is_some());
            if !claimed_tasks.is_empty() {
                components.create_action_row(|row| {
                    row.create_select_menu(|menu| {
                        menu.custom_id("unclaim-task")
                            .placeholder("Unclaim task")
                            .options(|opts| {
                                claimed_tasks.iter().for_each(|(task, _)| {
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

#[derive(Clone)]
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

    fn edit_interaction_response(
        self,
        r: &mut EditInteractionResponse,
    ) -> &mut EditInteractionResponse {
        r.content(self.content)
            .add_embed(self.embed)
            .set_components(self.components)
    }

    fn create_message<'a, 'b>(self, r: &'a mut CreateMessage<'b>) -> &'a mut CreateMessage<'b> {
        r.content(self.content)
            .set_embed(self.embed)
            .set_components(self.components)
    }
}
