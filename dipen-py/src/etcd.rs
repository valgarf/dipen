use std::time::Duration;

use dipen::etcd::ETCDConfig;
use pyo3::prelude::*;

#[pyclass(name = "ETCDConfig")]
pub struct PyETCDGateConfig {
    pub config: ETCDConfig,
}

#[pymethods]
impl PyETCDGateConfig {
    #[new]
    #[pyo3(signature = (endpoints, prefix="", node_name="default_node", region="default", lease_id=None, lease_ttl=Duration::from_secs(10), username=None, password=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        endpoints: Vec<String>,
        prefix: &str,
        node_name: &str,
        region: &str,
        lease_id: Option<i64>,
        lease_ttl: Duration,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Self {
        let mut client_options = etcd_client::ConnectOptions::new();
        if let (Some(u), Some(p)) = (username, password) {
            client_options = client_options.with_user(u, p)
        }
        Self {
            config: ETCDConfig {
                endpoints,
                prefix: prefix.into(),
                node_name: node_name.into(),
                region: region.into(),
                connect_options: Some(client_options),
                lease_id: lease_id.map(|lid| lid.into()),
                lease_ttl,
            },
        }
    }
}
