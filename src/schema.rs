use anyhow::Result;
use candid::types::TypeEnv;
use std::path::Path;

pub struct SchemaResolver {
    #[allow(dead_code)]
    pub type_env: TypeEnv,
}

impl SchemaResolver {
    pub fn load(did_path: Option<&Path>) -> Result<Self> {
        let type_env = match did_path {
            None => TypeEnv::new(),
            Some(path) => {
                let src = std::fs::read_to_string(path)
                    .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
                let prog = src
                    .parse::<candid_parser::IDLProg>()
                    .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;
                let mut env = TypeEnv::new();
                candid_parser::check_prog(&mut env, &prog)
                    .map_err(|e| anyhow::anyhow!("type error in {}: {e}", path.display()))?;
                env
            }
        };
        Ok(SchemaResolver { type_env })
    }
}
