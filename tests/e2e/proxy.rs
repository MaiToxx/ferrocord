//! Les variables de proxy ne doivent pas détourner les requêtes portant le
//! jeton. Pour ne pas modifier l'environnement du processus de test (non
//! sûr), le test se relance lui-même dans un processus enfant dont
//! l'environnement contient un proxy vers un port local fermé.

use crate::support::{ids::*, *};
use ferrocord::state::Event;

const CHILD: &str = "FERROCORD_TEST_PROXY_CHILD";
/// Port local fermé (discard) : si le proxy était utilisé, la requête échouerait.
const DEAD_PROXY: &str = "http://127.0.0.1:9";

#[test]
fn proxy_variables_are_ignored() {
    let exe = std::env::current_exe().unwrap_or_else(|e| panic!("exécutable de test : {e}"));
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["proxy::child_with_proxy_env", "--exact", "--test-threads=1"]).env(CHILD, "1");
    for var in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        cmd.env(var, DEAD_PROXY);
    }
    for var in ["NO_PROXY", "no_proxy"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().unwrap_or_else(|e| panic!("processus enfant : {e}"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "requête détournée par le proxy ?\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Ne fait rien hors du processus enfant lancé par `proxy_variables_are_ignored`.
#[tokio::test]
async fn child_with_proxy_env() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client
        .until("historique", |e| {
            assert!(!matches!(e, Event::HistoryError { .. }), "requête échouée : {e:?}");
            matches!(e, Event::History { .. })
        })
        .await;
    assert_eq!(s.http.requests().len(), 1);
}
