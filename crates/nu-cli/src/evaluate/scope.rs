use crate::prelude::*;
use crate::{commands::Command, whole_stream_command};
use nu_parser::ParserScope;
use nu_protocol::{hir::Block, Value};
use nu_source::Spanned;

#[derive(Debug, Clone)]
pub struct Scope {
    frames: Arc<parking_lot::Mutex<Vec<ScopeFrame>>>,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

impl Scope {
    pub fn new() -> Scope {
        Scope {
            frames: Arc::new(parking_lot::Mutex::new(vec![ScopeFrame::new()])),
        }
    }
    pub fn get_command(&self, name: &str) -> Option<Command> {
        for frame in self.frames.lock().iter().rev() {
            if let Some(command) = frame.get_command(name) {
                return Some(command);
            }
        }

        None
    }

    pub fn add_command(&self, name: String, command: Command) {
        // Note: this is assumed to always be true, as there is always a global top frame
        if let Some(frame) = self.frames.lock().last_mut() {
            frame.add_command(name, command)
        }
    }

    pub fn get_command_names(&self) -> Vec<String> {
        let mut names = vec![];

        for frame in self.frames.lock().iter() {
            let mut frame_command_names = frame.get_command_names();
            names.append(&mut frame_command_names);
        }

        names.dedup();
        names.sort();

        names
    }

    fn has_cmd_helper(&self, name: &str, f: fn(&ScopeFrame, &str) -> bool) -> bool {
        self.frames.lock().iter().any(|frame| f(frame, name))
    }

    pub fn has_command(&self, name: &str) -> bool {
        self.has_cmd_helper(name, ScopeFrame::has_command)
    }

    pub fn has_custom_command(&self, name: &str) -> bool {
        self.has_cmd_helper(name, ScopeFrame::has_custom_command)
    }

    pub fn has_alias(&self, name: &str) -> bool {
        self.has_cmd_helper(name, ScopeFrame::has_alias)
    }

    pub fn expect_command(&self, name: &str) -> Result<Command, ShellError> {
        if let Some(c) = self.get_command(name) {
            Ok(c)
        } else {
            Err(ShellError::untagged_runtime_error(format!(
                "Missing command '{}'",
                name
            )))
        }
    }

    pub fn get_vars(&self) -> IndexMap<String, Value> {
        //FIXME: should this be an interator?
        let mut output = IndexMap::new();

        for frame in self.frames.lock().iter().rev() {
            for v in frame.vars.iter() {
                output.insert(v.0.clone(), v.1.clone());
            }
        }

        output
    }

    pub fn get_env_vars(&self) -> IndexMap<String, String> {
        //FIXME: should this be an interator?
        let mut output = IndexMap::new();

        for frame in self.frames.lock().iter().rev() {
            for v in frame.env.iter() {
                output.insert(v.0.clone(), v.1.clone());
            }
        }

        output
    }

    pub fn get_var(&self, name: &str) -> Option<Value> {
        for frame in self.frames.lock().iter().rev() {
            if let Some(v) = frame.vars.get(name) {
                return Some(v.clone());
            }
        }

        None
    }

    pub fn add_var(&self, name: impl Into<String>, value: Value) {
        if let Some(frame) = self.frames.lock().last_mut() {
            frame.vars.insert(name.into(), value);
        }
    }

    pub fn add_vars(&self, vars: &IndexMap<String, Value>) {
        if let Some(frame) = self.frames.lock().last_mut() {
            frame
                .vars
                .extend(vars.iter().map(|(s, v)| (s.clone(), v.clone())))
        }
    }

    pub fn add_env_var(&self, name: impl Into<String>, value: String) {
        if let Some(frame) = self.frames.lock().last_mut() {
            frame.env.insert(name.into(), value);
        }
    }

    pub fn add_env(&self, env_vars: IndexMap<String, String>) {
        if let Some(frame) = self.frames.lock().last_mut() {
            frame.env.extend(env_vars)
        }
    }
}

impl ParserScope for Scope {
    fn get_signature(&self, name: &str) -> Option<nu_protocol::Signature> {
        self.get_command(name).map(|x| x.signature())
    }

    fn has_signature(&self, name: &str) -> bool {
        self.get_command(name).is_some()
    }

    fn add_definition(&self, block: Block) {
        if let Some(frame) = self.frames.lock().last_mut() {
            let name = block.params.name.clone();
            frame.custom_commands.insert(name.clone(), block.clone());
            frame.commands.insert(name, whole_stream_command(block));
        }
    }

    fn get_definitions(&self) -> Vec<Block> {
        let mut blocks = vec![];
        if let Some(frame) = self.frames.lock().last() {
            for (_, custom_command) in &frame.custom_commands {
                blocks.push(custom_command.clone());
            }
        }
        blocks
    }

    fn get_alias(&self, name: &str) -> Option<Vec<Spanned<String>>> {
        for frame in self.frames.lock().iter().rev() {
            if let Some(x) = frame.aliases.get(name) {
                return Some(x.clone());
            }
        }
        None
    }

    fn add_alias(&self, name: &str, replacement: Vec<Spanned<String>>) {
        // Note: this is assumed to always be true, as there is always a global top frame
        if let Some(frame) = self.frames.lock().last_mut() {
            frame.aliases.insert(name.to_string(), replacement);
        }
    }

    fn enter_scope(&self) {
        self.frames.lock().push(ScopeFrame::new());
    }

    fn exit_scope(&self) {
        self.frames.lock().pop();
    }
}

/// An evaluation scope. Scopes map variable names to Values and aid in evaluating blocks and expressions.
#[derive(Debug, Clone)]
pub struct ScopeFrame {
    pub vars: IndexMap<String, Value>,
    pub env: IndexMap<String, String>,
    pub commands: IndexMap<String, Command>,
    pub custom_commands: IndexMap<String, Block>,
    pub aliases: IndexMap<String, Vec<Spanned<String>>>,
}

impl ScopeFrame {
    pub fn has_command(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    pub fn has_custom_command(&self, name: &str) -> bool {
        self.custom_commands.contains_key(name)
    }

    pub fn has_alias(&self, name: &str) -> bool {
        self.aliases.contains_key(name)
    }

    pub fn get_command_names(&self) -> Vec<String> {
        self.commands.keys().map(|x| x.to_string()).collect()
    }

    pub fn add_command(&mut self, name: String, command: Command) {
        self.commands.insert(name, command);
    }

    pub fn get_command(&self, name: &str) -> Option<Command> {
        self.commands.get(name).cloned()
    }

    pub fn new() -> ScopeFrame {
        ScopeFrame {
            vars: IndexMap::new(),
            env: IndexMap::new(),
            commands: IndexMap::new(),
            custom_commands: IndexMap::new(),
            aliases: IndexMap::new(),
        }
    }
}

// impl Scope {
//     pub fn vars(&self) -> IndexMap<String, Value> {
//         //FIXME: should this be an interator?

//         let mut output = IndexMap::new();

//         for v in &self.vars {
//             output.insert(v.0.clone(), v.1.clone());
//         }

//         if let Some(parent) = &self.parent {
//             for v in parent.vars() {
//                 if !output.contains_key(&v.0) {
//                     output.insert(v.0.clone(), v.1.clone());
//                 }
//             }
//         }

//         output
//     }

//     pub fn env(&self) -> IndexMap<String, String> {
//         //FIXME: should this be an interator?

//         let mut output = IndexMap::new();

//         for v in &self.env {
//             output.insert(v.0.clone(), v.1.clone());
//         }

//         if let Some(parent) = &self.parent {
//             for v in parent.env() {
//                 if !output.contains_key(&v.0) {
//                     output.insert(v.0.clone(), v.1.clone());
//                 }
//             }
//         }

//         output
//     }

//     pub fn var(&self, name: &str) -> Option<Value> {
//         if let Some(value) = self.vars().get(name) {
//             Some(value.clone())
//         } else {
//             None
//         }
//     }

//     pub fn append_var(this: Arc<Self>, name: impl Into<String>, value: Value) -> Arc<Scope> {
//         let mut vars = IndexMap::new();
//         vars.insert(name.into(), value);
//         Arc::new(Scope {
//             vars,
//             env: IndexMap::new(),
//             commands: IndexMap::new(),
//             aliases: IndexMap::new(),
//             parent: Some(this),
//         })
//     }

//     pub fn append_vars(this: Arc<Self>, vars: IndexMap<String, Value>) -> Arc<Scope> {
//         Arc::new(Scope {
//             vars,
//             env: IndexMap::new(),
//             commands: IndexMap::new(),
//             aliases: IndexMap::new(),
//             parent: Some(this),
//         })
//     }

//     pub fn append_env(this: Arc<Self>, env: IndexMap<String, String>) -> Arc<Scope> {
//         Arc::new(Scope {
//             vars: IndexMap::new(),
//             env,
//             commands: IndexMap::new(),
//             aliases: IndexMap::new(),
//             parent: Some(this),
//         })
//     }

// }
