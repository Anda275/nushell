use nu_protocol::ast::Call;
use nu_protocol::engine::{Command, EngineState, Stack};
use nu_protocol::{
    record, Category, Example, IntoPipelineData, LazyRecord, PipelineData, Record, ShellError,
    Signature, Span, Type, Value,
};
use sysinfo::{Pid, PidExt, ProcessExt, ProcessRefreshKind, RefreshKind, System, SystemExt};
const ENV_PATH_SEPARATOR_CHAR: char = {
    #[cfg(target_family = "windows")]
    {
        ';'
    }
    #[cfg(not(target_family = "windows"))]
    {
        ':'
    }
};

#[derive(Clone)]
pub struct DebugInfo;

impl Command for DebugInfo {
    fn name(&self) -> &str {
        "debug info"
    }

    fn usage(&self) -> &str {
        "View process memory info."
    }

    fn extra_usage(&self) -> &str {
        "This command is meant for debugging purposes.\nIt shows you the process information and system memory information."
    }

    fn signature(&self) -> nu_protocol::Signature {
        Signature::build("debug info")
            .input_output_types(vec![(Type::Nothing, Type::Record(vec![]))])
            .category(Category::Debug)
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        _call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = Span::unknown();

        let record = LazySystemInfoRecord { span };

        Ok(Value::lazy_record(Box::new(record), span).into_pipeline_data())
    }

    fn examples(&self) -> Vec<Example> {
        vec![Example {
            description: "View process information",
            example: "debug info",
            result: None,
        }]
    }
}

#[derive(Debug, Clone)]
struct LazySystemInfoRecord {
    span: Span,
}

impl LazySystemInfoRecord {
    fn get_column_value_with_system(
        &self,
        column: &str,
        system_option: Option<&System>,
    ) -> Result<Value, ShellError> {
        let pid = Pid::from(std::process::id() as usize);
        match column {
            "thread_id" => Ok(Value::int(get_thread_id(), self.span)),
            "pid" => Ok(Value::int(pid.as_u32() as i64, self.span)),
            "ppid" => {
                // only get information requested
                let system_opt = SystemOpt::from((system_option, || {
                    RefreshKind::new().with_processes(
                        ProcessRefreshKind::new()
                            .without_cpu()
                            .without_disk_usage()
                            .without_user(),
                    )
                }));

                let system = system_opt.get_system();
                // get the process information for the nushell pid
                let pinfo = system.process(pid);

                Ok(pinfo
                    .and_then(|p| p.parent())
                    .map(|p| Value::int(p.as_u32() as i64, self.span))
                    .unwrap_or(Value::nothing(self.span)))
            }
            "system" => {
                // only get information requested
                let system_opt =
                    SystemOpt::from((system_option, || RefreshKind::new().with_memory()));

                let system = system_opt.get_system();

                Ok(Value::record(
                    record! {
                        "total_memory" => Value::filesize(system.total_memory() as i64, self.span),
                        "free_memory" => Value::filesize(system.free_memory() as i64, self.span),
                        "used_memory" => Value::filesize(system.used_memory() as i64, self.span),
                        "available_memory" => Value::filesize(system.available_memory() as i64, self.span),
                    },
                    self.span,
                ))
            }
            "process" => {
                // only get information requested
                let system_opt = SystemOpt::from((system_option, || {
                    RefreshKind::new().with_processes(
                        ProcessRefreshKind::new()
                            .without_cpu()
                            .without_disk_usage()
                            .without_user(),
                    )
                }));

                let system = system_opt.get_system();
                // get the process information for the nushell pid
                let pinfo = system.process(pid);

                if let Some(p) = pinfo {
                    Ok(Value::record(
                        record! {
                            "memory" => Value::filesize(p.memory() as i64, self.span),
                            "virtual_memory" => Value::filesize(p.virtual_memory() as i64, self.span),
                            "status" => Value::string(p.status().to_string(), self.span),
                            "root" => {
                                if let Some(filename) = p.exe().parent() {
                                    Value::string(filename.to_string_lossy().to_string(), self.span)
                                } else {
                                    Value::nothing(self.span)
                                }
                            },
                            "cwd" => Value::string(p.cwd().to_string_lossy().to_string(), self.span),
                            "exe_path" => Value::string(p.exe().to_string_lossy().to_string(), self.span),
                            "command" => Value::string(p.cmd().join(" "), self.span),
                            "name" => Value::string(p.name().to_string(), self.span),
                               "environment" => {
                                    let mut env_rec = Record::new();
                                    for val in p.environ() {
                                        if let Some((key, value)) = val.split_once('=') {
                                            let is_env_var_a_list = {
                                                {
                                                    #[cfg(target_family = "windows")]
                                                    {
                                                        key == "Path" || key == "PATHEXT" || key == "PSMODULEPATH" || key == "PSModulePath"
                                                    }
                                                    #[cfg(not(target_family = "windows"))]
                                                    {
                                                        key == "PATH" || key == "DYLD_FALLBACK_LIBRARY_PATH"
                                                    }
                                                }
                                            };
                                            if is_env_var_a_list {
                                                let items = value.split(ENV_PATH_SEPARATOR_CHAR).map(|r| Value::string(r.to_string(), self.span)).collect::<Vec<_>>();
                                                env_rec.push(key.to_string(), Value::list(items, self.span));
                                            } else if key == "LS_COLORS" { // LS_COLORS is a special case, it's a colon separated list of key=value pairs
                                                let items = value.split(':').map(|r| Value::string(r.to_string(), self.span)).collect::<Vec<_>>();
                                                env_rec.push(key.to_string(), Value::list(items, self.span));
                                            } else {
                                                env_rec.push(key.to_string(), Value::string(value.to_string(), self.span));
                                            }
                                        }
                                    }
                                    Value::record(env_rec, self.span)
                                },
                        },
                        self.span,
                    ))
                } else {
                    // If we can't get the process information, just return the system information
                    // only get information requested
                    let system_opt =
                        SystemOpt::from((system_option, || RefreshKind::new().with_memory()));
                    let system = system_opt.get_system();

                    Ok(Value::record(
                        record! {
                            "total_memory" => Value::filesize(system.total_memory() as i64, self.span),
                            "free_memory" => Value::filesize(system.free_memory() as i64, self.span),
                            "used_memory" => Value::filesize(system.used_memory() as i64, self.span),
                            "available_memory" => Value::filesize(system.available_memory() as i64, self.span),
                        },
                        self.span,
                    ))
                }
            }
            _ => Err(ShellError::IncompatibleParametersSingle {
                msg: format!("Unknown column: {}", column),
                span: self.span,
            }),
        }
    }
}

impl<'a> LazyRecord<'a> for LazySystemInfoRecord {
    fn column_names(&'a self) -> Vec<&'a str> {
        vec!["thread_id", "pid", "ppid", "process", "system"]
    }

    fn get_column_value(&self, column: &str) -> Result<Value, ShellError> {
        self.get_column_value_with_system(column, None)
    }

    fn span(&self) -> Span {
        self.span
    }

    fn clone_value(&self, span: Span) -> Value {
        Value::lazy_record(Box::new(LazySystemInfoRecord { span }), span)
    }

    fn collect(&'a self) -> Result<Value, ShellError> {
        let rk = RefreshKind::new()
            .with_processes(
                ProcessRefreshKind::new()
                    .without_cpu()
                    .without_disk_usage()
                    .without_user(),
            )
            .with_memory();
        // only get information requested
        let system = System::new_with_specifics(rk);

        self.column_names()
            .into_iter()
            .map(|col| {
                let val = self.get_column_value_with_system(col, Some(&system))?;
                Ok((col.to_owned(), val))
            })
            .collect::<Result<Record, _>>()
            .map(|record| Value::record(record, self.span()))
    }
}

enum SystemOpt<'a> {
    Ptr(&'a System),
    Owned(Box<System>),
}

impl<'a> SystemOpt<'a> {
    fn get_system(&'a self) -> &'a System {
        match self {
            SystemOpt::Ptr(system) => system,
            SystemOpt::Owned(system) => system,
        }
    }
}

impl<'a, F: Fn() -> RefreshKind> From<(Option<&'a System>, F)> for SystemOpt<'a> {
    fn from((system_opt, refresh_kind_create): (Option<&'a System>, F)) -> Self {
        match system_opt {
            Some(system) => SystemOpt::<'a>::Ptr(system),
            None => SystemOpt::Owned(Box::new(System::new_with_specifics(refresh_kind_create()))),
        }
    }
}

fn get_thread_id() -> i64 {
    #[cfg(target_family = "windows")]
    {
        unsafe { windows::Win32::System::Threading::GetCurrentThreadId() as i64 }
    }
    #[cfg(not(target_family = "windows"))]
    {
        unsafe { libc::pthread_self() as i64 }
    }
}
