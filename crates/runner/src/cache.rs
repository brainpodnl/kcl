use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};

use anyhow::{Result, anyhow};
use kcl_ast::ast::{Module, Program};
use kcl_parser::{KCLModuleCache, LoadProgramOptions, ParseSessionRef, load_program};

#[derive(PartialEq)]
struct Namespace {
    root: String,
    cwd: PathBuf,
    work_dir: String,
    vendor_dirs: Vec<String>,
    package_maps: HashMap<String, String>,
    load_plugins: bool,
}

#[derive(Default)]
struct State {
    namespace: Option<Namespace>,
    modules: KCLModuleCache,
}

/// Pristine parsed modules for one immutable source bundle. Clear or replace the
/// cache when sources change; evaluated values and resolver state are never retained.
#[derive(Default)]
pub struct ProgramCache {
    state: Mutex<State>,
}

impl ProgramCache {
    pub fn clear(&self) -> Result<()> {
        *self
            .state
            .lock()
            .map_err(|_| anyhow!("KCL program cache poisoned"))? = State::default();
        Ok(())
    }

    pub(crate) fn load(
        &self,
        sess: ParseSessionRef,
        paths: &[&str],
        opts: LoadProgramOptions,
    ) -> Result<Program> {
        let entries = kcl_parser::entry::get_compile_entries_from_paths(
            &paths
                .iter()
                .map(|path| path.to_string())
                .collect::<Vec<_>>(),
            &opts,
        )?;
        let namespace = Namespace {
            root: entries.get_root_path().to_string(),
            cwd: std::env::current_dir()?,
            work_dir: opts.work_dir.clone(),
            vendor_dirs: opts.vendor_dirs.clone(),
            package_maps: opts.package_maps.clone(),
            load_plugins: opts.load_plugins,
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("KCL program cache poisoned"))?;
        if state.namespace.as_ref() != Some(&namespace) {
            *state = State {
                namespace: Some(namespace),
                ..Default::default()
            };
        }
        let cached_modules = state
            .modules
            .read()
            .map_err(|_| anyhow!("KCL module cache poisoned"))?
            .ast_cache
            .len();
        let loaded = match load_program(sess, paths, Some(opts), Some(state.modules.clone())) {
            Ok(loaded) => loaded,
            Err(error) => {
                *state = State::default();
                return Err(error);
            }
        };
        if !loaded.errors.is_empty() {
            *state = State::default();
        }
        tracing::info!(
            stage = "kcl_module_cache",
            cached_modules,
            module_count = loaded.program.modules.len(),
            "deployment timing"
        );
        tracing::info_span!("deployment.kcl.clone_program").in_scope(|| {
            let started = Instant::now();
            let result = detach(loaded.program);
            tracing::info!(
                stage = "kcl_clone_program",
                elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
                success = result.is_ok(),
                "deployment timing"
            );
            result
        })
    }
}

fn detach(program: Program) -> Result<Program> {
    let mut cloned = HashMap::new();
    let mut clone_modules = |modules: HashMap<String, Arc<RwLock<Module>>>| {
        modules
            .into_iter()
            .map(|(path, module)| {
                let key = Arc::as_ptr(&module);
                let detached = match cloned.get(&key) {
                    Some(module) => Arc::clone(module),
                    None => {
                        let copy = module
                            .read()
                            .map_err(|_| anyhow!("KCL module poisoned"))?
                            .clone();
                        let copy = Arc::new(RwLock::new(copy));
                        cloned.insert(key, Arc::clone(&copy));
                        copy
                    }
                };
                Ok((path, detached))
            })
            .collect::<Result<HashMap<_, _>>>()
    };
    Ok(Program {
        root: program.root,
        pkgs: program.pkgs,
        pkgs_not_imported: program.pkgs_not_imported,
        modules: clone_modules(program.modules)?,
        modules_not_imported: clone_modules(program.modules_not_imported)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detaches_both_maps_and_preserves_internal_aliases() {
        let module = Arc::new(RwLock::new(Module {
            filename: "main.k".to_string(),
            ..Default::default()
        }));
        let program = Program {
            modules: HashMap::from([("main.k".to_string(), module.clone())]),
            modules_not_imported: HashMap::from([("alias.k".to_string(), module.clone())]),
            ..Default::default()
        };
        let copy = detach(program).unwrap();
        let main = &copy.modules["main.k"];
        assert!(!Arc::ptr_eq(main, &module));
        assert!(Arc::ptr_eq(main, &copy.modules_not_imported["alias.k"]));
        main.write().unwrap().filename = "changed.k".to_string();
        assert_eq!(module.read().unwrap().filename, "main.k");
    }
}
