use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{BuildHasher, BuildHasherDefault},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use clap::Parser;
use entity::{archive_rule, delivery, delivery_item, request, task, user};
use futures::FutureExt;
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
        EditInteractionResponse, EditMessage,
    },
    model::{
        application::{
            command::CommandOptionChoice,
            interaction::message_component::MessageComponentInteraction,
        },
        id::{ChannelId, MessageId},
        prelude::{
            interaction::{application_command::ApplicationCommandInteraction, Interaction},
            UserId,
        },
    },
    prelude::{EventHandler, GatewayIntents},
};
use slashery::{
    ArgFromInteractionError, SlashArg, SlashArgs, SlashCmd, SlashCmdType, SlashCmds,
    SlashComponents,
};
use snafu::{futures::TryFutureExt as _, OptionExt, Report, ResultExt, Snafu};
use strum::IntoEnumIterator;
use time::OffsetDateTime;

mod expiration_controller;
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
    "Sledges will tech in 15 minutes",
    "And our MPF champion is... CRIPPLING DEPRESSION!",
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
        RequestType::from_str(&arg).map_err(|err| {
            slashery::ArgFromInteractionError::InvalidValueForType {
                expected: serenity::model::application::command::CommandOptionType::String,
                got: arg.into(),
                message: Some(err.to_string()),
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
    /// How long the request should last for before becoming archived (examples: 1 min, 2 hours)
    expires_in: Option<HumanDuration>,
}

struct HumanDuration(Duration);

impl SlashArg for HumanDuration {
    fn arg_parse(
        arg: Option<&serenity::model::prelude::application_command::CommandDataOption>,
    ) -> Result<Self, slashery::ArgFromInteractionError> {
        let arg = String::arg_parse(arg)?;
        humantime::parse_duration(&arg).map(Self).map_err(|err| {
            ArgFromInteractionError::InvalidValueForType {
                expected: serenity::model::application::command::CommandOptionType::String,
                got: serde_json::Value::String(arg),
                message: Some(err.to_string()),
            }
        })
    }

    fn arg_discord_type() -> serenity::model::prelude::command::CommandOptionType {
        serenity::model::application::command::CommandOptionType::String
    }

    fn arg_required() -> bool {
        true
    }
}

#[derive(SlashCmd)]
#[slashery(name = "scopecreep", kind = "SlashCmdType::ChatInput")]
/// SCOPE CREEP
struct ScopeCreep {}

#[derive(SlashCmd)]
#[slashery(name = "delivery", kind = "SlashCmdType::ChatInput")]
/// Record a delivery made
struct MakeDelivery {
    /// The recipient of the delivery, such as WarEco or FEARS
    recipient: String,
    /// The amount of cmats pallets delivered
    cmats: Option<i32>,
    /// The amount of pcons pallets delivered
    pcons: Option<i32>,
    /// The amount of steel pallets delivered
    steel: Option<i32>,
    /// The amount of ass1 pallets delivered
    ass1: Option<i32>,
    /// The amount of ass2 pallets delivered
    ass2: Option<i32>,
    /// The amount of ass3 pallets delivered
    ass3: Option<i32>,
    /// The amount of ass4 pallets delivered
    ass4: Option<i32>,
    /// The amount of ass5 pallets delivered
    ass5: Option<i32>,
}

#[derive(SlashCmds)]
enum Cmd {
    MakeRequest(MakeRequest),
    ScopeCreep(ScopeCreep),
    MakeDelivery(MakeDelivery),
}

#[derive(SlashComponents)]
enum Component {
    // Legacy aliases because untyped generator used kebab-case ids
    #[slashery(id_alias("unclaim-task"))]
    UnclaimTask,
    #[slashery(id_alias("claim-task"))]
    ClaimTask,
    #[slashery(id_alias("complete-task"))]
    CompleteTask,
    #[slashery(id_alias("repeat-request"))]
    RepeatRequest,
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
            Interaction::ApplicationCommand(cmd) => match Cmd::from_interaction(&cmd) {
                Ok(Cmd::MakeRequest(req)) => self.make_request(cmd, req, ctx).await,
                Ok(Cmd::MakeDelivery(req)) => self.make_delivery(cmd, req, ctx).await,
                Ok(Cmd::ScopeCreep(req)) => self.scope_creep(cmd, req, ctx).await,
                Err(err) => cmd
                    .create_interaction_response(&ctx, |r| {
                        r.interaction_response_data(|r| {
                            r.ephemeral(true).content(Report::from_error(err))
                        })
                    })
                    .await
                    .unwrap(),
            },
            Interaction::MessageComponent(comp) => {
                match Component::from_interaction(&comp).unwrap() {
                    Component::UnclaimTask => {
                        self.update_request_task_status(comp, ctx, TaskState::Unclaimed)
                            .await
                    }
                    Component::ClaimTask => {
                        self.update_request_task_status(comp, ctx, TaskState::Claimed)
                            .await
                    }
                    Component::CompleteTask => {
                        self.update_request_task_status(comp, ctx, TaskState::Completed)
                            .await
                    }
                    Component::RepeatRequest => self.repeat_request(comp, ctx).await,
                }
            }
            _ => (),
        }
    }
}

impl Handler {
    async fn scope_creep(
        &self,
        cmd: ApplicationCommandInteraction,
        _req: ScopeCreep,
        ctx: serenity::prelude::Context,
    ) {
        let url = "https://cdn.discordapp.com/attachments/1144367081740042380/1186582003676622848/IMG_7437.gif";
        cmd.create_interaction_response(&ctx.http, |r| {
            r.interaction_response_data(|r| r.content(url))
        })
        .await
        .unwrap();
    }

    async fn make_delivery(
        &self,
        cmd: ApplicationCommandInteraction,
        req: MakeDelivery,
        ctx: serenity::prelude::Context,
    ) {
        let user = get_user_by_discord(&self.db, cmd.user.id).await.unwrap();
        let delivery = delivery::ActiveModel {
            created_by: Set(user.id),
            ..Default::default()
        }
        .insert(&self.db)
        .await
        .unwrap();
        let delivered_items = [
            (req.cmats, "cmats"),
            (req.pcons, "pcons"),
            (req.steel, "steel"),
            (req.ass1, "assmat 1"),
            (req.ass2, "assmat 2"),
            (req.ass3, "assmat 3"),
            (req.ass4, "assmat 4"),
            (req.ass5, "assmat 5"),
        ]
        .into_iter()
        .filter_map(|(amount, item_name)| Some((amount?, item_name)))
        .collect::<Vec<_>>();
        delivery_item::Entity::insert_many(delivered_items.iter().map(|(amount, item_name)| {
            delivery_item::ActiveModel {
                delivery: Set(delivery.id),
                item_name: Set(item_name.to_string()),
                amount: Set(*amount),
                ..Default::default()
            }
        }))
        .exec(&self.db)
        .await
        .unwrap();
        cmd.create_interaction_response(&ctx.http, |r| {
            r.interaction_response_data(|r| {
                use std::fmt::Write;
                let mut content = format!(
                    "# New delivery by <@{from}> to **{to}**",
                    from = cmd.user.id,
                    to = req.recipient
                );
                for (amount, item_name) in delivered_items {
                    write!(content, "\n- {amount} pallets of {item_name}").unwrap();
                }
                r.content(content)
            })
        })
        .await
        .unwrap();

        let response_message = cmd.get_interaction_response(&ctx.http).await.unwrap();
        delivery::ActiveModel {
            discord_message_id: Set(Some(response_message.id.0 as i64)),
            ..delivery.into()
        }
        .update(&self.db)
        .await
        .unwrap();
    }

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
            discord_guild_id: Set(cmd.guild_id.map(|g| g.0 as i64)),
            thumbnail_url: Set(req.kind.thumbnail().map(str::to_string)),
            expires_on: Set(req
                .expires_in
                .map(|expires_in| OffsetDateTime::now_utc() + expires_in.0)),
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

        match archive_request_if_required(&self.db, request_id, Some(&comp), &ctx).await {
            Ok(ArchiveResult::Archived) => return,
            Err(err) => tracing::error!(
                error = &err as &dyn std::error::Error,
                request.id = %request_id,
                "failed to process whether to archive request, ignoring..."
            ),
            _ => (),
        }

        let rendered = render_request(&self.db, request_id).await;
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
            expires_on: Set(original_request.expires_on.map(|expires_on| {
                OffsetDateTime::now_utc() + (expires_on - original_request.created_at)
            })),
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

#[derive(PartialEq, Eq)]
enum ArchiveResult {
    Archived,
    AlreadyArchived,
    NotReadyToArchiveYet,
}

#[derive(Debug, Snafu)]
#[snafu(module)]
enum ArchiveRequestError {
    Database {
        source: DbErr,
    },
    #[snafu(display("request {request} not found"))]
    RequestNotFound {
        request: Uuid,
    },
    #[snafu(display("request {request} is missing discord channel id ({discord_channel_id:?}) or message id ({discord_message_id:?})"))]
    RequestMissingDiscordInfo {
        request: Uuid,
        discord_message_id: Option<i64>,
        discord_channel_id: Option<i64>,
    },
    GetDiscordChannelInfo {
        source: serenity::Error,
        channel: ChannelId,
    },
    DiscordChannelHasNoGuild {
        channel: ChannelId,
    },
    DiscordSendArchivedRequestMessage {
        source: serenity::Error,
        channel: ChannelId,
    },
    DiscordSendArchivedRequestNotification {
        source: serenity::Error,
    },
    DiscordDeleteRequestMessage {
        source: serenity::Error,
    },
    DiscordEditRequestMessage {
        source: serenity::Error,
    },
}

async fn archive_request_if_required(
    db: &DatabaseConnection,
    request_id: Uuid,
    comp: Option<&MessageComponentInteraction>,
    discord: &impl serenity::http::CacheHttp,
) -> Result<ArchiveResult, ArchiveRequestError> {
    use archive_request_error::*;
    let request = request::Entity::find_by_id(request_id)
        .one(db)
        .await
        .context(DatabaseSnafu)?
        .context(RequestNotFoundSnafu {
            request: request_id,
        })?;
    let (message_id, from_channel) = if let Some(comp) = comp {
        (comp.message.id, comp.channel_id)
    } else {
        let (message_id, channel_id) = (request.discord_message_id.zip(request.discord_channel_id))
            .context(RequestMissingDiscordInfoSnafu {
                request: request_id,
                discord_message_id: request.discord_message_id,
                discord_channel_id: request.discord_channel_id,
            })?;
        (MessageId(message_id as u64), ChannelId(channel_id as u64))
    };
    if request.archived_on.is_some() {
        return Ok(ArchiveResult::AlreadyArchived);
    }
    let tasks = request
        .find_related(task::Entity)
        .all(db)
        .await
        .context(DatabaseSnafu)?;
    let request_completed = request
        .expires_on
        .map_or(false, |e| e < OffsetDateTime::now_utc())
        || tasks.iter().all(|t| t.completed_at.is_some());
    let archive_channel = if request_completed {
        archive_rule::Entity::find_by_id(from_channel.0 as i64)
            .one(db)
            .await
            .context(DatabaseSnafu)?
            .map(|rule| ChannelId(rule.to_channel as u64))
    } else {
        return Ok(ArchiveResult::NotReadyToArchiveYet);
    };

    // mark request as archived
    request::ActiveModel {
        id: sea_orm::ActiveValue::Unchanged(request_id),
        archived_on: Set(Some(OffsetDateTime::now_utc())),
        ..Default::default()
    }
    .update(db)
    .await
    .context(DatabaseSnafu)?;

    // try to move request to archive channel, otherwise archive in-place
    if let Some(archive_channel) = archive_channel {
        let archive_channel = archive_channel
            .to_channel(discord)
            .await
            .context(GetDiscordChannelInfoSnafu {
                channel: archive_channel,
            })?
            .guild()
            .context(DiscordChannelHasNoGuildSnafu {
                channel: archive_channel,
            })?;
        let rendered = render_request(db, request_id).await;
        let archived_msg = archive_channel
            .send_message(discord.http(), |msg| rendered.create_message(msg))
            .await
            .context(DiscordSendArchivedRequestMessageSnafu {
                channel: archive_channel,
            })?;
        if let Some(comp) = comp {
            comp.create_interaction_response(discord.http(), |msg| {
                msg.interaction_response_data(|r| {
                    r.ephemeral(true).content(format!(
                        "Request has been archived, see {}",
                        archived_msg.link()
                    ))
                })
            })
            .await
            .context(DiscordSendArchivedRequestNotificationSnafu)?;
        }
        // apparently the interaction message counts as a followup, which should avoid
        // requiring permission to see the channel
        if let Some(comp) = comp {
            comp.delete_followup_message(&discord.http(), comp.message.id)
                .await
                .context(DiscordDeleteRequestMessageSnafu)?;
        } else {
            from_channel
                .delete_message(&discord.http(), message_id)
                .await
                .context(DiscordDeleteRequestMessageSnafu)?;
        }
        request::ActiveModel {
            id: sea_orm::ActiveValue::Unchanged(request_id),
            discord_message_id: Set(Some(archived_msg.id.0 as i64)),
            ..Default::default()
        }
        .update(db)
        .await
        .context(DatabaseSnafu)?;
    } else {
        let rendered = render_request(db, request_id).await;
        if let Some(comp) = comp {
            comp.edit_original_message(&discord.http(), |r| {
                rendered.create_interaction_response(r)
            })
            .await
            .context(DiscordEditRequestMessageSnafu)?;
        } else {
            from_channel
                .edit_message(&discord.http(), message_id, |r| rendered.edit_message(r))
                .await
                .context(DiscordEditRequestMessageSnafu)?;
        }
    }

    Ok(ArchiveResult::Archived)
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env()
                .whatever_context("invalid log filter")?,
        )
        .init();
    let opts = Opts::parse();
    let db = Database::connect(opts.database_url)
        .await
        .whatever_context("failed to connect to database")?;
    migration::Migrator::up(&db, None)
        .await
        .whatever_context("failed to apply migrations")?;
    let mut discord = serenity::Client::builder(&opts.discord_token, GatewayIntents::GUILDS)
        .application_id(opts.discord_app_id)
        .event_handler(Handler { db: db.clone() })
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
    let discord_ctx = Arc::clone(&discord.cache_and_http);
    futures::future::select_ok([
        discord
            .start()
            .whatever_context("failed to run discord bot")
            .boxed_local(),
        expiration_controller::run(&db, &discord_ctx)
            .map(Ok)
            .boxed_local(),
    ])
    .await?;
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
        content: [
            Some(format!("# {}\n", request.title)),
            request.archived_on.map(|archived_on| {
                format!(
                    "Archived on <t:{ts}> (<t:{ts}:R>)\n",
                    ts = archived_on.unix_timestamp()
                )
            }),
            request.expires_on.map(|expires_on| {
                format!(
                    "Expires on <t:{ts}> (<t:{ts}:R>)\n",
                    ts = expires_on.unix_timestamp()
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect::<String>(),
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
            let uncompleted_tasks = if request.archived_on.is_none() {
                tasks
                    .iter()
                    .filter(|(task, _)| task.completed_at.is_none())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let (claimed_tasks, unclaimed_tasks) = uncompleted_tasks
                .iter()
                .copied()
                .partition::<Vec<_>, _>(|(task, _)| task.started_at.is_some());
            if !claimed_tasks.is_empty() {
                components.create_action_row(|row| {
                    row.create_select_menu(|menu| {
                        menu.custom_id(Component::UnclaimTask.component_id())
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
                        menu.custom_id(Component::ClaimTask.component_id())
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
                        menu.custom_id(Component::CompleteTask.component_id())
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
                    row.create_button(|button| {
                        button
                            .custom_id(Component::RepeatRequest.component_id())
                            .label("Repeat")
                    })
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

    fn edit_message<'a, 'b>(self, r: &'a mut EditMessage<'b>) -> &'a mut EditMessage<'b> {
        r.content(self.content)
            .set_embed(self.embed)
            .set_components(self.components)
    }
}
