//! `spektr verify <case>`: exit 0 when everything checks out, 3 at the
//! first problem (ARCHITECTURE §10).

use anyhow::{Result, bail};
use serde_json::json;
use spektr_store::Db;

use crate::cli::VerifyArgs;
use crate::wiring::Ctx;

/// Returns whether the case verified.
pub fn run(ctx: &Ctx, a: VerifyArgs) -> Result<bool> {
    if !a.case.exists() {
        bail!("{} not found", a.case.display());
    }
    let mut db = Db::open_read_only(&a.case, ctx.store_options()?)?;
    let r = db.verify()?;
    if a.json {
        println!(
            "{}",
            json!({
                "ok": r.ok(), "audit_rows": r.audit_rows, "blobs": r.blobs, "observations": r.observations,
                "failure": r.failure.as_ref().map(|f| json!({ "location": f.location.to_string(), "reason": f.reason })),
            })
        );
    } else {
        match &r.failure {
            None => println!(
                "✓ {} rows · {} blobs · {} observations",
                r.audit_rows, r.blobs, r.observations
            ),
            Some(f) => println!("✕ {}: {}", f.location, f.reason),
        }
    }
    Ok(r.ok())
}
