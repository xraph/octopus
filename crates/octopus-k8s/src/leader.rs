//! Leader election via a `coordination.k8s.io/v1` Lease.
//!
//! When several operator replicas run for HA, only the lease holder should drive
//! the watchers (otherwise every replica would fight to reconcile the same
//! resources). Each replica races to acquire a single namespaced Lease; the
//! holder renews it periodically, and a replica may take over once the holder's
//! lease expires. The acquire/renew *decision* ([`can_acquire`]) is pure and
//! unit-tested; the Kubernetes I/O loop is in [`run_leader_loop`].

use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::MicroTime;
use k8s_openapi::chrono::{DateTime, Duration, Utc};
use kube::api::{Api, ObjectMeta, Patch, PatchParams};
use kube::Client;

/// Lease object name.
pub const LEASE_NAME: &str = "octopus-operator";
/// How long a lease is valid without renewal.
pub const LEASE_DURATION_SECS: i64 = 15;
/// How often the holder renews (and challengers re-check).
pub const RENEW_INTERVAL_SECS: u64 = 5;

/// The parts of a Lease the acquire decision reasons about.
#[derive(Clone, Debug, Default)]
pub struct LeaseRecord {
    /// Current holder identity (`spec.holderIdentity`).
    pub holder: Option<String>,
    /// Last renewal time (`spec.renewTime`).
    pub renew_time: Option<DateTime<Utc>>,
    /// Lease validity window (`spec.leaseDurationSeconds`).
    pub lease_duration_secs: Option<i64>,
}

/// Decide whether identity `me` may acquire or renew the lease, given the
/// `current` lease state (if any) and the current time `now`.
///
/// Acquirable when: there is no lease, it has no/empty holder, we already hold
/// it (renewal), or the holder's lease has expired (`renewTime + duration`
/// elapsed). A live lease held by someone else is not acquirable.
pub fn can_acquire(current: Option<&LeaseRecord>, now: DateTime<Utc>, me: &str) -> bool {
    let Some(record) = current else {
        return true; // no lease yet
    };
    match record.holder.as_deref() {
        None | Some("") => true,    // unheld
        Some(h) if h == me => true, // we hold it: renew
        Some(_) => match (record.renew_time, record.lease_duration_secs) {
            // Held by someone else: acquirable only once their lease has lapsed.
            (Some(renew), Some(dur)) => now > renew + Duration::seconds(dur),
            // Missing timing info → treat as stale and acquirable.
            _ => true,
        },
    }
}

/// This replica's identity for the lease (the pod name via the downward API,
/// falling back to a process-unique string for local runs).
pub fn pod_identity() -> String {
    std::env::var("POD_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("octopus-{}", std::process::id()))
}

/// Namespace the Lease lives in (the pod's namespace via the downward API).
pub fn lease_namespace() -> String {
    std::env::var("POD_NAMESPACE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

/// Read the current lease into a [`LeaseRecord`], or `None` if it doesn't exist.
async fn read_lease(api: &Api<Lease>) -> kube::Result<Option<LeaseRecord>> {
    Ok(api.get_opt(LEASE_NAME).await?.map(|l| {
        let spec = l.spec.unwrap_or_default();
        LeaseRecord {
            holder: spec.holder_identity,
            renew_time: spec.renew_time.map(|t| t.0),
            lease_duration_secs: spec.lease_duration_seconds.map(i64::from),
        }
    }))
}

/// Server-side-apply the lease with `me` as holder and `now` as the renew time.
async fn claim_lease(
    api: &Api<Lease>,
    namespace: &str,
    me: &str,
    now: DateTime<Utc>,
) -> kube::Result<()> {
    let lease = Lease {
        metadata: ObjectMeta {
            name: Some(LEASE_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(LeaseSpec {
            holder_identity: Some(me.to_string()),
            lease_duration_seconds: Some(LEASE_DURATION_SECS as i32),
            renew_time: Some(MicroTime(now)),
            acquire_time: Some(MicroTime(now)),
            ..Default::default()
        }),
    };
    let params = PatchParams::apply("octopus-operator").force();
    api.patch(LEASE_NAME, &params, &Patch::Apply(&lease)).await?;
    Ok(())
}

/// Continuously acquire/renew the lease on every [`RENEW_INTERVAL_SECS`] tick.
///
/// This never returns; spawn it as a background task after
/// [`acquire_leadership`] has confirmed this replica is the leader. While we
/// hold the lease each tick renews it; if we lose it the loop keeps trying to
/// re-acquire once the new holder's lease lapses.
pub async fn run_leader_loop(client: Client, namespace: String, identity: String) {
    let api: Api<Lease> = Api::namespaced(client, &namespace);
    loop {
        let now = Utc::now();
        match read_lease(&api).await {
            Ok(current) => {
                if can_acquire(current.as_ref(), now, &identity) {
                    if let Err(e) = claim_lease(&api, &namespace, &identity, now).await {
                        tracing::warn!(error = %e, "failed to claim/renew operator lease");
                    }
                } else {
                    tracing::debug!(holder = ?current.and_then(|c| c.holder), "operator lease held by another replica");
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to read operator lease"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(RENEW_INTERVAL_SECS)).await;
    }
}

/// Block until this replica acquires the lease, polling every
/// [`RENEW_INTERVAL_SECS`].
pub async fn acquire_leadership(client: &Client, namespace: &str, identity: &str) {
    let api: Api<Lease> = Api::namespaced(client.clone(), namespace);
    loop {
        let now = Utc::now();
        match read_lease(&api).await {
            Ok(current) if can_acquire(current.as_ref(), now, identity) => {
                match claim_lease(&api, namespace, identity, now).await {
                    Ok(()) => {
                        tracing::info!(identity, "acquired operator leadership");
                        return;
                    }
                    Err(e) => tracing::warn!(error = %e, "lease claim failed; retrying"),
                }
            }
            Ok(_) => tracing::info!("waiting for operator leadership"),
            Err(e) => tracing::warn!(error = %e, "failed to read operator lease"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(RENEW_INTERVAL_SECS)).await;
    }
}

#[cfg(test)]
#[allow(clippy::needless_pass_by_value)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn acquirable_when_no_lease_exists() {
        assert!(can_acquire(None, ts("2026-01-01T00:00:00Z"), "me"));
    }

    #[test]
    fn acquirable_when_holder_empty() {
        let r = LeaseRecord {
            holder: None,
            ..Default::default()
        };
        assert!(can_acquire(Some(&r), ts("2026-01-01T00:00:00Z"), "me"));
    }

    #[test]
    fn renewable_when_we_already_hold_it() {
        let r = LeaseRecord {
            holder: Some("me".into()),
            renew_time: Some(ts("2026-01-01T00:00:00Z")),
            lease_duration_secs: Some(15),
        };
        // Even before expiry, the current holder may renew.
        assert!(can_acquire(Some(&r), ts("2026-01-01T00:00:05Z"), "me"));
    }

    #[test]
    fn not_acquirable_while_other_holder_is_live() {
        let r = LeaseRecord {
            holder: Some("other".into()),
            renew_time: Some(ts("2026-01-01T00:00:00Z")),
            lease_duration_secs: Some(15),
        };
        // 10s < 15s window: still live.
        assert!(!can_acquire(Some(&r), ts("2026-01-01T00:00:10Z"), "me"));
    }

    #[test]
    fn acquirable_after_other_holders_lease_expires() {
        let r = LeaseRecord {
            holder: Some("other".into()),
            renew_time: Some(ts("2026-01-01T00:00:00Z")),
            lease_duration_secs: Some(15),
        };
        // 20s > 15s window: expired, may take over.
        assert!(can_acquire(Some(&r), ts("2026-01-01T00:00:20Z"), "me"));
    }
}
