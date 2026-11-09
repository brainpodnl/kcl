use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use anyhow::{Result, anyhow};
use kcl_ast::ast::Program;
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
        mut opts: LoadProgramOptions,
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
        opts.detach_cached_modules = true;
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
        Ok(loaded.program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn cached_modules_are_detached_before_import_normalization_and_resolution() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("kcl.mod"), "").unwrap();
        std::fs::create_dir(dir.path().join("pkg")).unwrap();
        std::fs::write(dir.path().join("pkg/value.k"), "value = 1\n").unwrap();
        let main = dir.path().join("main.k");
        std::fs::write(&main, "import pkg\nresult = pkg.value\n").unwrap();
        let cache = ProgramCache::default();
        let program = cache
            .load(
                Default::default(),
                &[main.to_str().unwrap()],
                LoadProgramOptions {
                    work_dir: dir.path().display().to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        let state = cache.state.lock().unwrap();
        let modules = state.modules.read().unwrap();
        for (name, module) in &program.modules {
            let original = &modules.ast_cache[&PathBuf::from(name)];
            assert!(!Arc::ptr_eq(original, module));
            let pristine = original.read().unwrap().clone();
            module.write().unwrap().body.clear();
            assert_eq!(*original.read().unwrap(), pristine);
        }
    }
}
