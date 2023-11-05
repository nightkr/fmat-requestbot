use std::{
    fmt::Display,
    future::Future,
    sync::{Arc, Mutex},
};

// pub async fn report_command_result<
//     E: Display,
//     D: ToString,
//     Fut: Future<Output = Result<D, E>>,
//     F: FnOnce(Arc<Mutex<bool>>) -> Fut,
// >(
//     ctx: &serenity::client::Context,
//     cmd: &ApplicationCommandInteraction,
//     f: F,
// ) {
//     let interacted = Arc::new(Mutex::new(false));
//     let result = f(interacted.clone()).await;
//     match result {
//         Ok(msg) => {
//             if *interacted.lock().await {
//                 let _ = cmd
//                     .edit_original_interaction_response(&ctx.http, |res| res.content(msg))
//                     .await;
//             } else {
//                 let _ = cmd
//                     .create_interaction_response(&ctx.http, |res| {
//                         res.interaction_response_data(|res| res.content(msg))
//                     })
//                     .await;
//             }
//         }
//         Err(err) => {
//             if *interacted.lock().await {
//                 let _ = cmd
//                     .edit_original_interaction_response(&ctx.http, |res| {
//                         res.content(format!("Failed to execute command: {:#}", err))
//                     })
//                     .await;
//             } else {
//                 let _ = cmd
//                     .create_interaction_response(&ctx.http, |res| {
//                         res.interaction_response_data(|res| {
//                             res.content(format!("Failed to execute command: {:#}", err))
//                                 .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
//                         })
//                     })
//                     .await;
//             }
//         }
//     };
// }
