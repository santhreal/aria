use aria_sdk::{Host, LogLevel, Plugin};

const COMMAND_ID: &str = "__COMMAND_ID__";
const COMMAND_TITLE: &str = "__COMMAND_TITLE__";

#[derive(Debug, Default)]
pub struct SamplePlugin;

impl Plugin for SamplePlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.log(LogLevel::Info, "mounted __PLUGIN_ID__");
        host.register_command(COMMAND_ID, COMMAND_TITLE)?;
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        match id {
            COMMAND_ID => {
                host.log(LogLevel::Info, "ran __COMMAND_ID__");
                host.storage_set("last_run", COMMAND_ID.as_bytes())?;
                Ok(())
            }
            other => Err(format!("unknown command: {other}")),
        }
    }
}

aria_sdk::export!(SamplePlugin);
