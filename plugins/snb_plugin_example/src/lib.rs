//! Example plugin demonstrating Shinobu's plugin capabilities.
//!
//! Registers sample commands (`/echo`, `/ping`), hooks, a message handler,
//! session-based multi-turn echo, and a demo async adapter — all declared with
//! the `#[command]` / `#[hook]` / `#[message_handler]` / `#[adapter]` macros and
//! auto-registered via [`register_all`](snb_core::context::register_all).

use std::sync::Arc;
use std::time::Duration;

use snb_core::command::CommandContext;
use snb_core::context::BotContext;
use snb_core::event::Event;
use snb_core::hook::HookType;
use snb_core::session::{SessionKey, SessionMessage, SessionState};
use snb_macros::{adapter, command, hook, message_handler, plugin};

// -- Commands ----------------------------------------------------------------

#[command(name = "echo", aliases = ["say"])]
fn echo(ctx: &CommandContext) -> anyhow::Result<()> {
    let bot = snb_core::context::bot();
    let msg = ctx.event.message.as_ref();

    // No args → enter session "echo" mode, wait for next message.
    if ctx.args.is_empty() {
        if let (Some(chat_id), Some(user_id)) = (
            msg.and_then(|m| m.to.as_deref()),
            msg.and_then(|m| m.from.as_deref()),
        ) {
            let key = SessionKey::private(chat_id, user_id);
            let sm = bot.get_session_manager();
            sm.append_message(&key, SessionMessage::system("echo"));
            sm.set_state(&key, SessionState::WaitingForInput);
        }
        let mut resp = Event::message("MyPlugin", "Send me a message to echo.");
        if let Some(m) = msg {
            resp.message.as_mut().unwrap().to = m.to.clone();
            resp.message.as_mut().unwrap().reply_to = m.id.clone();
        }
        if let Some(sender) = &ctx.event.sender {
            resp.receiver = Some(sender.clone());
        }
        bot.emit_event(resp);
        return Ok(());
    }

    // Has args → echo directly.
    log::info!(
        "[command] /echo args={:?} source={} from={:?} to={:?}",
        ctx.args,
        ctx.event.source,
        msg.and_then(|m| m.from.as_deref()),
        msg.and_then(|m| m.to.as_deref()),
    );
    let mut resp = Event::message("MyPlugin", ctx.args);
    if let Some(m) = msg {
        resp.message.as_mut().unwrap().to = m.to.clone();
        resp.message.as_mut().unwrap().reply_to = m.id.clone();
    }
    if let Some(sender) = &ctx.event.sender {
        resp.receiver = Some(sender.clone());
    }
    bot.emit_event(resp);
    Ok(())
}

#[command(name = "ping")]
fn ping(ctx: &CommandContext) -> anyhow::Result<()> {
    log::info!("[command] /ping source={}", ctx.event.source);
    let mut resp = Event::message("MyPlugin", "pong!");
    if let Some(msg) = &ctx.event.message {
        resp.message.as_mut().unwrap().to = msg.to.clone();
        resp.message.as_mut().unwrap().reply_to = msg.id.clone();
    }
    if let Some(sender) = &ctx.event.sender {
        resp.receiver = Some(sender.clone());
    }
    snb_core::context::bot().emit_event(resp);
    Ok(())
}

// -- Message Handlers --------------------------------------------------------

#[message_handler(name = "echo_handler")]
fn echo_handler(event: &Event) -> anyhow::Result<()> {
    let Some(msg) = &event.message else {
        return Ok(());
    };
    let text = msg.text();

    // Check if this user is in "echo" session mode.
    if let (Some(chat_id), Some(user_id)) = (msg.to.as_deref(), msg.from.as_deref()) {
        let key = SessionKey::private(chat_id, user_id);
        let sm = snb_core::context::bot().get_session_manager();
        if sm.get_state(&key) == SessionState::WaitingForInput {
            let recent = sm.get_all_messages(&key);
            if recent
                .last()
                .is_some_and(|m| m.role == "system" && m.content == "echo")
            {
                // Echo the user's message and exit session mode.
                let mut resp = Event::message("MyPlugin", &text);
                resp.message.as_mut().unwrap().to = msg.to.clone();
                resp.message.as_mut().unwrap().reply_to = msg.id.clone();
                if let Some(sender) = &event.sender {
                    resp.receiver = Some(sender.clone());
                }
                snb_core::context::bot().emit_event(resp);
                sm.clear_session(&key);
                return Ok(());
            }
        }
    }

    log::info!(
        "[message] text={:?} from={:?} to={:?} at={:?} chat_type={:?} source={}",
        text,
        msg.from,
        msg.to,
        msg.at,
        msg.chat_type,
        event.source,
    );
    Ok(())
}

// -- Hooks -------------------------------------------------------------------

#[hook(name = "log_hook", kind = HookType::All)]
fn log_hook(event: &mut Event) -> anyhow::Result<()> {
    log::debug!("event: {:?} | source: {}", event.event_type, event.source);
    if let Some(msg) = event.message.as_mut()
        && msg.text() == "hook"
    {
        msg.content = vec![snb_core::event::ContentItem::text("hooked")];
    }
    Ok(())
}

/// Demonstrates [`HookType::BeforeNamedCommand`]: rewrites the args of
/// `/echo` only, before the command runs.
#[hook(name = "echo_arg_rewrite", kind = HookType::BeforeNamedCommand("echo".to_string()))]
fn echo_arg_rewrite(event: &mut Event) -> anyhow::Result<()> {
    if let Some(cmd) = event.command.as_mut()
        && cmd.args == "rewrite"
    {
        cmd.args = "rewritten by hook".to_string();
    }
    Ok(())
}

// -- Demo async adapter ------------------------------------------------------

#[adapter]
async fn demo_tick(bot: Arc<dyn BotContext>) {
    for i in 1..=3 {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        bot.emit_event(Event::message("demo-tick", format!("tick {i}")));
    }
}

// -- Plugin ------------------------------------------------------------------

#[plugin(name = "MyPlugin", version = "0.1.0", kind = Plugin)]
struct MyPlugin;

// -- Unit test ---------------------------------------------------------------

#[test]
fn test_plugin_ffi() {
    use snb_core::plugin::Version;
    use std::ffi::CStr;

    let ptr = create_plugin();
    let cell = unsafe { snb_core::plugin::PluginCell::new(ptr, destroy_plugin, Box::new(())) };

    assert_eq!(cell.name(), "MyPlugin");
    assert_eq!(
        cell.version(),
        Version {
            major: 0,
            minor: 1,
            patch: 0,
        }
    );

    let abi = unsafe { CStr::from_ptr(plugin_abi()).to_str().unwrap() };
    assert_eq!(abi, "0.1.0");
}
