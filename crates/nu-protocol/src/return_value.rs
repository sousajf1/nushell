use crate::value::Value;
use nu_errors::ShellError;
use nu_source::{b, DebugDocBuilder, PrettyDebug, Tagged};
use serde::{Deserialize, Serialize};

/// The inner set of actions for the command processor. Each denotes a way to change state in the processor without changing it directly from the command itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandAction {
    /// Change to a new directory or path (in non-filesystem situations)
    ChangePath(String),
    /// Exit out of Nu
    Exit,
    /// Display an error
    Error(ShellError),
    /// Enter a new shell at the given path
    EnterShell(String),
    /// Convert the value given from one type to another
    AutoConvert(Value, String),
    /// Enter a value shell, one that allows exploring inside of a Value
    EnterValueShell(Value),
    /// Enter the help shell, which allows exploring the help system
    EnterHelpShell(Value),
    /// Add a variable into scope
    AddVariable(String, Value),
    /// Add an environment variable into scope
    AddEnvVariable(String, String),
    /// Add plugins from path given
    AddPlugins(String),
    /// Run the given script in the current context (given filename)
    SourceScript(Tagged<String>),
    /// Go to the previous shell in the shell ring buffer
    PreviousShell,
    /// Go to the next shell in the shell ring buffer
    NextShell,
    /// Leave the current shell. If it's the last shell, exit out of Nu
    LeaveShell,
}

impl PrettyDebug for CommandAction {
    /// Get a command action ready to be pretty-printed
    fn pretty(&self) -> DebugDocBuilder {
        match self {
            CommandAction::ChangePath(path) => b::typed("change path", b::description(path)),
            CommandAction::Exit => b::description("exit"),
            CommandAction::Error(_) => b::error("error"),
            CommandAction::AutoConvert(_, extension) => {
                b::typed("auto convert", b::description(extension))
            }
            CommandAction::EnterShell(s) => b::typed("enter shell", b::description(s)),
            CommandAction::EnterValueShell(v) => b::typed("enter value shell", v.pretty()),
            CommandAction::EnterHelpShell(v) => b::typed("enter help shell", v.pretty()),
            CommandAction::AddVariable(..) => b::description("add variable"),
            CommandAction::AddEnvVariable(..) => b::description("add environment variable"),
            CommandAction::SourceScript(..) => b::description("source script"),
            CommandAction::AddPlugins(..) => b::description("add plugins"),
            CommandAction::PreviousShell => b::description("previous shell"),
            CommandAction::NextShell => b::description("next shell"),
            CommandAction::LeaveShell => b::description("leave shell"),
        }
    }
}

/// The fundamental success type in the pipeline. Commands return these values as their main responsibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReturnSuccess {
    /// A value to be used or shown to the user
    Value(Value),
    /// A debug-enabled value to be used or shown to the user
    DebugValue(Value),
    /// An action to be performed as values pass out of the command. These are performed rather than passed to the next command in the pipeline
    Action(CommandAction),
}

impl PrettyDebug for ReturnSuccess {
    /// Get a return success ready to be pretty-printed
    fn pretty(&self) -> DebugDocBuilder {
        match self {
            ReturnSuccess::Value(value) => b::typed("value", value.pretty()),
            ReturnSuccess::DebugValue(value) => b::typed("debug value", value.pretty()),
            ReturnSuccess::Action(action) => b::typed("action", action.pretty()),
        }
    }
}

/// The core Result type for pipelines
pub type ReturnValue = Result<ReturnSuccess, ShellError>;

impl From<Value> for ReturnValue {
    fn from(v: Value) -> Self {
        Ok(ReturnSuccess::Value(v))
    }
}

impl ReturnSuccess {
    /// Get to the contained Value, if possible
    pub fn raw_value(&self) -> Option<Value> {
        match self {
            ReturnSuccess::Value(raw) => Some(raw.clone()),
            ReturnSuccess::DebugValue(raw) => Some(raw.clone()),
            ReturnSuccess::Action(_) => None,
        }
    }

    /// Helper function for an action to change the the path
    pub fn change_cwd(path: String) -> ReturnValue {
        Ok(ReturnSuccess::Action(CommandAction::ChangePath(path)))
    }

    /// Helper function to create simple values for returning
    pub fn value(input: impl Into<Value>) -> ReturnValue {
        Ok(ReturnSuccess::Value(input.into()))
    }

    /// Helper function to create simple debug-enabled values for returning
    pub fn debug_value(input: impl Into<Value>) -> ReturnValue {
        Ok(ReturnSuccess::DebugValue(input.into()))
    }

    /// Helper function for creating actions
    pub fn action(input: CommandAction) -> ReturnValue {
        Ok(ReturnSuccess::Action(input))
    }
}
