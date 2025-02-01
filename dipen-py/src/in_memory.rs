use dipen::storage::in_memory::{InMemoryConfigBuilder, InMemoryStorageClient};
use pyo3::prelude::*;

#[pyclass(name = "InMemoryStorageClient")]
#[derive(Clone)]
pub struct PyInMemoryStorageClient {
    pub storage: InMemoryStorageClient,
}

#[pymethods]
impl PyInMemoryStorageClient {
    #[new]
    #[pyo3(signature = (prefix="", node_name="default_node", region="default"))]
    #[allow(clippy::too_many_arguments)]
    fn new(prefix: &str, node_name: &str, region: &str) -> Self {
        let config = InMemoryConfigBuilder::default()
            .prefix(prefix)
            .node_name(node_name)
            .region(region)
            .build()
            .unwrap();

        PyInMemoryStorageClient { storage: InMemoryStorageClient::new(config) }
    }

    #[pyo3(signature = (prefix="", node_name="default_node", region="default"))]
    #[allow(clippy::too_many_arguments)]
    fn clone_with_config(&self, prefix: &str, node_name: &str, region: &str) -> Self {
        let config = InMemoryConfigBuilder::default()
            .prefix(prefix)
            .node_name(node_name)
            .region(region)
            .build()
            .unwrap();

        PyInMemoryStorageClient { storage: self.storage.clone_with_config(config) }
    }
}
