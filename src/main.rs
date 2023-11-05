use clap::Parser;
use serenity::{
    builder::{CreateEmbed, CreateSelectMenuOption},
    model::{
        application::interaction::message_component::MessageComponentInteraction,
        prelude::interaction::{application_command::ApplicationCommandInteraction, Interaction},
    },
    prelude::{EventHandler, GatewayIntents},
};
use slashery::{SlashArgs, SlashCmd, SlashCmdType, SlashCmds};
use snafu::ResultExt;

mod utils;

#[derive(clap::Parser)]
struct Opts {
    #[clap(long, env)]
    discord_token: String,
    #[clap(long, env)]
    discord_app_id: u64,
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

struct Handler;

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
                "complete-task" => self.complete_request_task(comp, ctx).await,
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
        let subrequests = req.tasks.split(';');
        cmd.create_interaction_response(&ctx.http, |m| {
            m.interaction_response_data(|m| {
                let mut tasks = CreateEmbed::default();
                tasks.title("Tasks");
                let task_items = subrequests
                    .into_iter()
                    .enumerate()
                    .map(|(i, subreq)| format!("{}. {subreq}", i + 1))
                    .collect::<Vec<_>>();
                tasks.description(task_items.join("\n"));
                m.content(format!("Request by <@{}>: {}", cmd.user.id, req.title))
                    .add_embed(tasks)
                    .components(|c| {
                        c.create_action_row(|r| {
                            r.create_select_menu(|m| {
                                m.custom_id("complete-task")
                                    .placeholder("Mark task as completed")
                                    .options(|o| {
                                        o.set_options(
                                            task_items
                                                .iter()
                                                .map(|t| {
                                                    let mut opt = CreateSelectMenuOption::default();
                                                    opt.value(t).label(t);
                                                    opt
                                                })
                                                .collect(),
                                        )
                                    })
                            })
                        })
                    })
            })
        })
        .await
        .unwrap();
    }

    async fn complete_request_task(
        &self,
        mut comp: MessageComponentInteraction,
        ctx: serenity::prelude::Context,
    ) {
        // dbg!(&comp.data);
        let tasks_embed = comp
            .message
            .embeds
            .iter_mut()
            .find(|em| em.title.as_deref() == Some("Tasks"))
            .expect("message has no tasks");
        if let Some(tasks_desc) = &mut tasks_embed.description {
            *tasks_desc = tasks_desc
                .lines()
                .map(|line| {
                    if comp.data.values.iter().any(|v| *v == line) {
                        if let Some((i, task)) = line.split_once(". ") {
                            format!("{i}. ~~{task}~~ completed by <@{}>", comp.user.id)
                        } else {
                            line.to_string()
                        }
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<String>>()
                .join("\n");
        }
        let task_items_full = tasks_embed.description.clone().unwrap_or_default();
        let options: Vec<_> = task_items_full
            .lines()
            .filter(|t| !t.contains("~~"))
            .map(|t| {
                let mut opt = CreateSelectMenuOption::default();
                opt.value(t).label(t);
                opt
            })
            .collect();
        comp.edit_original_message(&ctx.http, |m| {
            m.interaction_response_data(|m| {
                m.set_embeds(comp.message.embeds.iter().cloned().map(CreateEmbed::from))
                    .components(|c| {
                        if !options.is_empty() {
                            c.create_action_row(|r| {
                                r.create_select_menu(|m| {
                                    m.custom_id("complete-task")
                                        .placeholder("Mark task as completed")
                                        .options(|o| o.set_options(options))
                                })
                            })
                        } else {
                            c
                        }
                    })
            })
        })
        .await
        .unwrap();
        // tasks_embed
        //     .description
        //     .unwrap_or_default()
        //     .lines()
        //     .collect::<Vec>();
        // for completed_task in &comp.data.values {
        //     tasks_embed
        // }
    }
}

#[snafu::report]
#[tokio::main]
async fn main() -> Result<(), snafu::Whatever> {
    let opts = Opts::parse();
    let mut discord = serenity::Client::builder(&opts.discord_token, GatewayIntents::empty())
        .application_id(opts.discord_app_id)
        .event_handler(Handler)
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
