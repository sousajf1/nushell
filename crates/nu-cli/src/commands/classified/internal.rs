use std::sync::atomic::Ordering;

use crate::commands::UnevaluatedCallInfo;
use crate::prelude::*;
use log::{log_enabled, trace};
use nu_errors::ShellError;
use nu_protocol::hir::{ExternalRedirection, InternalCommand};
use nu_protocol::{CommandAction, Primitive, ReturnSuccess, UntaggedValue, Value};

pub(crate) async fn run_internal_command(
    command: InternalCommand,
    context: &EvaluationContext,
    input: InputStream,
) -> Result<InputStream, ShellError> {
    if log_enabled!(log::Level::Trace) {
        trace!(target: "nu::run::internal", "->");
        trace!(target: "nu::run::internal", "{}", command.name);
    }

    let objects: InputStream = trace_stream!(target: "nu::trace_stream::internal", "input" = input);

    let internal_command = context.scope.expect_command(&command.name);

    if command.name == "autoenv untrust" {
        context
            .user_recently_used_autoenv_untrust
            .store(true, Ordering::SeqCst);
    }

    let result = {
        context
            .run_command(
                internal_command?,
                Tag::unknown_anchor(command.name_span),
                command.args.clone(),
                objects,
            )
            .await?
    };

    let head = Arc::new(command.args.head.clone());
    let context = context.clone();
    let command = Arc::new(command);

    Ok(InputStream::from_stream(
        result
            .then(move |item| {
                let head = head.clone();
                let command = command.clone();
                let context = context.clone();
                async move {
                    match item {
                        Ok(ReturnSuccess::Action(action)) => match action {
                            CommandAction::ChangePath(path) => {
                                context.shell_manager.set_path(path);
                                InputStream::empty()
                            }
                            CommandAction::Exit => std::process::exit(0), // TODO: save history.txt
                            CommandAction::Error(err) => {
                                context.error(err);
                                InputStream::empty()
                            }
                            CommandAction::AutoConvert(tagged_contents, extension) => {
                                let contents_tag = tagged_contents.tag.clone();
                                let command_name = format!("from {}", extension);
                                let command = command.clone();
                                if let Some(converter) = context.scope.get_command(&command_name) {
                                    let new_args = RawCommandArgs {
                                        host: context.host.clone(),
                                        ctrl_c: context.ctrl_c.clone(),
                                        current_errors: context.current_errors.clone(),
                                        shell_manager: context.shell_manager.clone(),
                                        call_info: UnevaluatedCallInfo {
                                            args: nu_protocol::hir::Call {
                                                head: (&*head).clone(),
                                                positional: None,
                                                named: None,
                                                span: Span::unknown(),
                                                external_redirection: ExternalRedirection::Stdout,
                                            },
                                            name_tag: Tag::unknown_anchor(command.name_span),
                                        },
                                        scope: context.scope.clone(),
                                    };
                                    let result = converter
                                        .run(new_args.with_input(vec![tagged_contents]))
                                        .await;

                                    match result {
                                        Ok(mut result) => {
                                            let result_vec: Vec<Result<ReturnSuccess, ShellError>> =
                                                result.drain_vec().await;

                                            let mut output = vec![];
                                            for res in result_vec {
                                                match res {
                                                    Ok(ReturnSuccess::Value(Value {
                                                        value: UntaggedValue::Table(list),
                                                        ..
                                                    })) => {
                                                        for l in list {
                                                            output.push(Ok(l));
                                                        }
                                                    }
                                                    Ok(ReturnSuccess::Value(Value {
                                                        value,
                                                        ..
                                                    })) => {
                                                        output
                                                            .push(Ok(value
                                                                .into_value(contents_tag.clone())));
                                                    }
                                                    Err(e) => output.push(Err(e)),
                                                    _ => {}
                                                }
                                            }

                                            futures::stream::iter(output).to_input_stream()
                                        }
                                        Err(err) => {
                                            context.error(err);
                                            InputStream::empty()
                                        }
                                    }
                                } else {
                                    InputStream::one(tagged_contents)
                                }
                            }
                            CommandAction::EnterHelpShell(value) => match value {
                                Value {
                                    value: UntaggedValue::Primitive(Primitive::String(cmd)),
                                    tag,
                                } => {
                                    context.shell_manager.insert_at_current(Box::new(
                                        match HelpShell::for_command(
                                            UntaggedValue::string(cmd).into_value(tag),
                                            &context.scope,
                                        ) {
                                            Ok(v) => v,
                                            Err(err) => {
                                                context.error(err);
                                                return InputStream::empty();
                                            }
                                        },
                                    ));
                                    InputStream::from_stream(futures::stream::iter(vec![]))
                                }
                                _ => {
                                    context.shell_manager.insert_at_current(Box::new(
                                        match HelpShell::index(&context.scope) {
                                            Ok(v) => v,
                                            Err(err) => {
                                                context.error(err);
                                                return InputStream::empty();
                                            }
                                        },
                                    ));
                                    InputStream::from_stream(futures::stream::iter(vec![]))
                                }
                            },
                            CommandAction::EnterValueShell(value) => {
                                context
                                    .shell_manager
                                    .insert_at_current(Box::new(ValueShell::new(value)));
                                InputStream::from_stream(futures::stream::iter(vec![]))
                            }
                            CommandAction::EnterShell(location) => {
                                context.shell_manager.insert_at_current(Box::new(
                                    match FilesystemShell::with_location(location) {
                                        Ok(v) => v,
                                        Err(err) => {
                                            context.error(err.into());
                                            return InputStream::empty();
                                        }
                                    },
                                ));
                                InputStream::from_stream(futures::stream::iter(vec![]))
                            }
                            CommandAction::AddVariable(name, value) => {
                                context.scope.add_var(name, value);
                                InputStream::from_stream(futures::stream::iter(vec![]))
                            }
                            CommandAction::AddEnvVariable(name, value) => {
                                context.scope.add_env_var(name, value);
                                InputStream::from_stream(futures::stream::iter(vec![]))
                            }
                            CommandAction::SourceScript(filename) => {
                                let contents = std::fs::read_to_string(&filename.item);
                                match contents {
                                    Ok(contents) => {
                                        let result = crate::script::run_script_standalone(
                                            contents, true, &context, false,
                                        )
                                        .await;

                                        if let Err(err) = result {
                                            context.error(err.into());
                                        }
                                        InputStream::empty()
                                    }
                                    Err(_) => {
                                        context.error(ShellError::labeled_error(
                                            "Can't load file to source",
                                            "can't load file",
                                            filename.span(),
                                        ));

                                        InputStream::empty()
                                    }
                                }
                            }
                            CommandAction::AddPlugins(path) => {
                                match crate::plugin::scan(vec![std::path::PathBuf::from(path)]) {
                                    Ok(plugins) => {
                                        context.add_commands(
                                            plugins
                                                .into_iter()
                                                .filter(|p| {
                                                    !context.is_command_registered(p.name())
                                                })
                                                .collect(),
                                        );

                                        InputStream::empty()
                                    }
                                    Err(reason) => {
                                        context.error(reason);
                                        InputStream::empty()
                                    }
                                }
                            }
                            CommandAction::PreviousShell => {
                                context.shell_manager.prev();
                                InputStream::empty()
                            }
                            CommandAction::NextShell => {
                                context.shell_manager.next();
                                InputStream::empty()
                            }
                            CommandAction::LeaveShell => {
                                context.shell_manager.remove_at_current();
                                if context.shell_manager.is_empty() {
                                    std::process::exit(0); // TODO: save history.txt
                                }
                                InputStream::empty()
                            }
                        },

                        Ok(ReturnSuccess::Value(Value {
                            value: UntaggedValue::Error(err),
                            ..
                        })) => {
                            context.error(err);
                            InputStream::empty()
                        }

                        Ok(ReturnSuccess::Value(v)) => InputStream::one(v),

                        Ok(ReturnSuccess::DebugValue(v)) => {
                            let doc = PrettyDebug::pretty_doc(&v);
                            let mut buffer = termcolor::Buffer::ansi();

                            let _ = doc.render_raw(
                                context.with_host(|host| host.width() - 5),
                                &mut nu_source::TermColored::new(&mut buffer),
                            );

                            let value = String::from_utf8_lossy(buffer.as_slice());

                            InputStream::one(UntaggedValue::string(value).into_untagged_value())
                        }

                        Err(err) => {
                            context.error(err);
                            InputStream::empty()
                        }
                    }
                }
            })
            .flatten()
            .take_while(|x| futures::future::ready(!x.is_error())),
    ))
}
