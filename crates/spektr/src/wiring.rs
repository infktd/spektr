//! Wiring (ARCHITECTURE §4): build `Net`, the engine config, register the
//! built-ins and exec modules.

use std::path::Path;

use anyhow::{Context, Result};
use spektr_engine::{CaseInit, Engine, EngineConfig, EngineHandle, OpenOptions};
use spektr_model::{Clock, IdGen};
use spektr_modules::net::{DnsConfig, Transport};
use spektr_modules::{BuiltinOptions, Fixtures, Net};

use crate::config::Config;
use crate::paths::Paths;
use crate::testenv::{TestEnv, TransportSpec};

/// Which modules a command registers.
pub enum Modules<'a> {
    All,
    Only(&'a [String]),
    /// Read-only commands: no modules at all.
    None,
}

/// Everything a command needs from the environment.
pub struct Ctx {
    pub paths: Paths,
    pub config: Config,
    pub test: TestEnv,
    pub secrets: std::collections::BTreeMap<String, String>,
}

impl Ctx {
    pub fn load() -> Result<Self> {
        let test = TestEnv::from_env()?;
        let paths = Paths::resolve()?;
        let config = Config::load(&paths.config_file())?;
        let secrets = config.secrets(std::env::vars());
        Ok(Self {
            paths,
            config,
            test,
            secrets,
        })
    }

    pub fn store_options(&self) -> Result<OpenOptions> {
        let clock = self.test.clock.map_or(Clock::System, Clock::Fixed);
        let ids = self.test.seed.map_or_else(IdGen::random, IdGen::seeded);
        Ok(OpenOptions::new(clock, ids)?)
    }

    pub fn net(&self) -> Result<Net> {
        let transport = match &self.test.transport {
            TransportSpec::Live => Transport::Live,
            TransportSpec::Record(dir) => Transport::Record(dir.clone()),
            TransportSpec::Replay(dir) => {
                let fx = Fixtures::load_dir(dir)
                    .with_context(|| format!("loading fixtures from {}", dir.display()))?;
                return Ok(Net::replay(fx));
            }
        };
        let dns = DnsConfig {
            nameservers: self.config.dns.nameservers.clone(),
            ..DnsConfig::default()
        };
        Ok(Net::new(transport, dns)?)
    }

    pub fn engine_config(&self) -> Result<EngineConfig> {
        let mut c = EngineConfig::new(self.store_options()?);
        c.config_dir = Some(self.paths.config.clone());
        c.secrets = self.secrets.clone();
        c.module_config = self.config.module_config();
        Ok(c)
    }

    /// Register built-ins and exec modules, then start.
    pub fn start(&self, engine: Engine, modules: Modules<'_>) -> Result<EngineHandle> {
        let wanted = |name: &str| match modules {
            Modules::All => true,
            Modules::Only(list) => list.iter().any(|o| o == name),
            Modules::None => false,
        };
        if !matches!(modules, Modules::None) {
            let net = self.net()?;
            let opts = BuiltinOptions {
                config_dir: Some(self.paths.config.clone()),
            };
            for m in spektr_modules::builtins_with(net, opts) {
                if wanted(&m.manifest().name) {
                    engine.register(m)?;
                }
            }
            engine.load_exec_modules(&self.paths.modules_dir());
        }
        Ok(engine.start())
    }

    pub async fn open(
        &self,
        case: &Path,
        depth: Option<u32>,
        modules: Modules<'_>,
    ) -> Result<EngineHandle> {
        let mut cfg = self.engine_config()?;
        cfg.depth_override = depth;
        let engine = Engine::open(case, cfg)
            .await
            .with_context(|| format!("opening {}", case.display()))?;
        self.start(engine, modules)
    }

    pub async fn create(&self, case: &Path, init: &CaseInit) -> Result<EngineHandle> {
        let engine = Engine::create(case, init, self.engine_config()?).await?;
        self.start(engine, Modules::None)
    }
}
