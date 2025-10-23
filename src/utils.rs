use std::borrow::Cow;

/// Returns a string that begins the same as `value`, but is at most `length_limit`
/// bytes long.
///
/// If `value` is too long to fit then it will end with an ellipsis.
fn ellipsize_string<'a>(value: &'a str, length_limit: usize) -> Cow<'a, str> {
    const ELLIPSIS: &str = "…";
    assert!(
        length_limit >= ELLIPSIS.len(),
        "length_limit must at least fit the ellipsis ({})",
        ELLIPSIS.len()
    );

    if value.len() <= length_limit {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(format!(
            "{}{ELLIPSIS}",
            &value[..value.floor_char_boundary(length_limit - ELLIPSIS.len())]
        ))
    }
}

#[test]
fn test_ellipsize_string() {
    assert_eq!(ellipsize_string("hey there!", 6), "hey…");
    assert_eq!(ellipsize_string("hello", 5), "hello");
    assert_eq!(ellipsize_string("hello", 10), "hello");
}

/// Ellipsizes a string value to fit within a Discord dropdown interaction.
pub fn ellipsize_discord_dropdown_value<'a>(value: &'a str) -> Cow<'a, str> {
    ellipsize_string(
        value,
        // taken from https://discord.com/developers/docs/components/reference#string-select-select-option-structure
        100,
    )
}

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
