use anyhow::Result;
use candid::types::{Label, Type, TypeEnv, TypeInner};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct SchemaResolver {
    #[allow(dead_code)]
    pub type_env: TypeEnv,
    pub hash_to_name: HashMap<u32, String>,
}

impl SchemaResolver {
    pub fn load(did_paths: &[&Path]) -> Result<Self> {
        let mut env = TypeEnv::new();
        for path in did_paths {
            let src = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
            let prog = src
                .parse::<candid_parser::IDLProg>()
                .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;
            candid_parser::check_prog(&mut env, &prog)
                .map_err(|e| anyhow::anyhow!("type error in {}: {e}", path.display()))?;
        }
        let hash_to_name = build_hash_map(&env);
        Ok(SchemaResolver {
            type_env: env,
            hash_to_name,
        })
    }
}

fn build_hash_map(env: &TypeEnv) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let mut visited = HashSet::new();
    for name in env.0.keys().cloned().collect::<Vec<_>>() {
        collect_from_named(&name, env, &mut map, &mut visited);
    }
    map
}

fn collect_from_named(
    name: &str,
    env: &TypeEnv,
    map: &mut HashMap<u32, String>,
    visited: &mut HashSet<String>,
) {
    if !visited.insert(name.to_string()) {
        return;
    }
    if let Some(ty) = env.0.get(name).cloned() {
        collect_labels(&ty, env, map, visited);
    }
}

fn collect_labels(
    ty: &Type,
    env: &TypeEnv,
    map: &mut HashMap<u32, String>,
    visited: &mut HashSet<String>,
) {
    match ty.as_ref() {
        TypeInner::Record(fields) | TypeInner::Variant(fields) => {
            for field in fields {
                if let Label::Named(n) = field.id.as_ref() {
                    map.insert(candid::idl_hash(n), n.clone());
                }
                collect_labels(&field.ty, env, map, visited);
            }
        }
        TypeInner::Opt(t) | TypeInner::Vec(t) => collect_labels(t, env, map, visited),
        TypeInner::Func(f) => {
            for t in &f.args {
                collect_labels(t, env, map, visited);
            }
            for t in &f.rets {
                collect_labels(t, env, map, visited);
            }
        }
        TypeInner::Service(methods) => {
            for (_, t) in methods {
                collect_labels(t, env, map, visited);
            }
        }
        TypeInner::Var(n) => collect_from_named(n, env, map, visited),
        _ => {}
    }
}
