//! One plugin's JavaScript engine, running on a dedicated thread.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use boa_engine::builtins::promise::PromiseState;
use boa_engine::module::IdleModuleLoader;
use boa_engine::object::builtins::JsPromise;
use boa_engine::{Context, JsObject, JsValue, Module, Source, js_string};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::host::{self, HOST, HostState};
use crate::{Options, PluginError, PluginMeta};

/// Stack for plugin threads. The JavaScript engine uses the native stack
/// for each nested call, and running out aborts the whole app, so this must
/// comfortably hold `Limits::recursion` nested calls. Only the memory
/// actually used is committed.
const PLUGIN_STACK_BYTES: usize = 64 * 1024 * 1024;

/// Which slots a plugin fills, from its exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slots {
    pub catalog: bool,
    pub resolver: bool,
    pub lyrics: bool,
}

/// A request to run one exported function.
pub struct Job {
    pub function: &'static str,
    pub args: Vec<Value>,
    pub reply: oneshot::Sender<Result<Value, PluginError>>,
}

/// Start a plugin thread. `ready` receives the plugin's meta once its code
/// has been evaluated, or the error that stopped it.
pub fn spawn(
    code: Arc<str>,
    options: Arc<Options>,
    ready: oneshot::Sender<Result<(PluginMeta, Slots), PluginError>>,
) -> std::io::Result<mpsc::Sender<Job>> {
    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    thread::Builder::new()
        .name("veena-plugin".into())
        .stack_size(PLUGIN_STACK_BYTES)
        .spawn(move || {
            let mut runtime = match Runtime::new(&code, &options) {
                Ok(runtime) => runtime,
                Err(e) => {
                    let _ = ready.send(Err(e));
                    return;
                }
            };
            let _ = ready.send(Ok((runtime.meta.clone(), runtime.slots)));
            for job in jobs_rx {
                let result = runtime.call(job.function, job.args);
                let _ = job.reply.send(result);
            }
        })?;
    Ok(jobs_tx)
}

struct Runtime {
    context: Context,
    exports: JsObject,
    meta: PluginMeta,
    slots: Slots,
}

impl Runtime {
    fn new(code: &str, options: &Options) -> Result<Self, PluginError> {
        let mut context = Context::builder()
            // Plugins are a single file: `import` is rejected.
            .module_loader(Rc::new(IdleModuleLoader))
            .build()
            .map_err(|e| PluginError::Load(e.to_string()))?;
        let limits = context.runtime_limits_mut();
        limits.set_loop_iteration_limit(options.limits.loop_iterations);
        limits.set_recursion_limit(options.limits.recursion);

        HOST.with(|h| {
            *h.borrow_mut() = Some(HostState::new("loading".into(), options.limits.clone()))
        });
        host::install(&mut context, &options.config)
            .map_err(|e| PluginError::Load(format!("host setup failed: {e}")))?;

        let module = Module::parse(Source::from_bytes(code.as_bytes()), None, &mut context)
            .map_err(|e| PluginError::Load(e.to_string()))?;
        let promise = module.load_link_evaluate(&mut context);
        settle(&promise, &mut context).map_err(PluginError::Load)?;
        let exports = module.namespace(&mut context);

        let meta = exports
            .get(js_string!("meta"), &mut context)
            .map_err(|e| PluginError::InvalidMeta(e.to_string()))?;
        let meta = meta
            .to_json(&mut context)
            .map_err(|e| PluginError::InvalidMeta(e.to_string()))?
            .ok_or_else(|| PluginError::InvalidMeta("`meta` is not exported".into()))?;
        let meta: PluginMeta =
            serde_json::from_value(meta).map_err(|e| PluginError::InvalidMeta(e.to_string()))?;
        meta.validate()?;

        let mut exported = |name: &str| -> Result<bool, PluginError> {
            Ok(exports
                .get(js_string!(name), &mut context)
                .map_err(|e| PluginError::Load(e.to_string()))?
                .is_callable())
        };
        let slots = Slots {
            catalog: exported("search")?,
            resolver: exported("resolve")?,
            lyrics: exported("lyrics")?,
        };
        if !slots.catalog && !slots.resolver && !slots.lyrics {
            return Err(PluginError::NothingProvided);
        }

        let domains = options
            .domains
            .clone()
            .unwrap_or_else(|| meta.domains.clone());
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let state = h.as_mut().expect("installed above");
            state.plugin = meta.id.clone();
            state.domains = domains;
        });

        Ok(Self {
            context,
            exports,
            meta,
            slots,
        })
    }

    /// Call an exported function and wait for its result.
    fn call(&mut self, function: &'static str, args: Vec<Value>) -> Result<Value, PluginError> {
        let context = &mut self.context;
        let export = self
            .exports
            .get(js_string!(function), context)
            .map_err(|e| PluginError::Script(e.to_string()))?;
        let Some(callable) = export.as_callable() else {
            return Err(PluginError::MissingExport(function));
        };

        HOST.with(|h| h.borrow_mut().as_mut().expect("installed").requests = 0);
        let args = args
            .iter()
            .map(|arg| JsValue::from_json(arg, context))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| PluginError::Script(e.to_string()))?;

        let mut result = callable
            .call(&JsValue::undefined(), &args, context)
            .map_err(|e| PluginError::Script(concise(&e.to_string())))?;
        if let Some(promise) = result
            .as_object()
            .and_then(|o| JsPromise::from_object(o).ok())
        {
            result = settle(&promise, context).map_err(PluginError::Script)?;
        }

        Ok(result
            .to_json(context)
            .map_err(|e| PluginError::InvalidResult(e.to_string()))?
            .unwrap_or(Value::Null))
    }
}

/// Keep the message and the first stack frames of an error.
fn concise(message: &str) -> String {
    message.lines().take(4).collect::<Vec<_>>().join("\n")
}

/// Run pending jobs until the promise settles; return its value.
fn settle(promise: &JsPromise, context: &mut Context) -> Result<JsValue, String> {
    context.run_jobs().map_err(|e| concise(&e.to_string()))?;
    match promise.state() {
        PromiseState::Fulfilled(value) => Ok(value),
        PromiseState::Rejected(reason) => Err(concise(&describe(&reason, context))),
        PromiseState::Pending => Err(
            "promise never settled (an engine limit such as the loop or recursion limit may have \
             stopped it; call untrusted code through veena.attempt)"
                .into(),
        ),
    }
}

/// Human-readable text for a thrown value.
fn describe(value: &JsValue, context: &mut Context) -> String {
    if let Some(object) = value.as_object()
        && let Ok(message) = object.get(js_string!("message"), context)
        && message.is_string()
    {
        let name = object
            .get(js_string!("name"), context)
            .ok()
            .filter(JsValue::is_string)
            .map(|n| n.display().to_string().trim_matches('"').to_owned())
            .unwrap_or_else(|| "Error".into());
        let message = message.display().to_string();
        return format!("{name}: {}", message.trim_matches('"'));
    }
    value.display().to_string()
}
