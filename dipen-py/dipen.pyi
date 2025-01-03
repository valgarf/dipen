# Configure the rust logging from the 'tracing' package to be logged via loguru
from datetime import timedelta
from enum import Enum

class RustTracingToLoguru:
    def __init__(self) -> None: ...
    # Set default log level (active for packages not defined via 'set_target_log_level')
    log_level: str
    # Set log level for a rust package prefix
    def set_target_log_level(self, prefix: str, level: str) -> None: ...
    def get_target_log_level(self, prefix: str) -> str | None: ...
    # Installs the tracing subscriber. Can only be called once during the lifetime of the current
    # process
    def install(self) -> None: ...

def run(net: PetriNetBuilder, etcd: ETCDGate) -> None: ...

class ArcVariant(Enum):
    In = ...
    Out = ...
    InOut = ...
    Cond = ...
    OutCond = ...

class PetriNetBuilder:
    def __init__(self) -> None: ...
    def insert_place(self, name: str, output_locking: bool = True): ...
    def insert_transition(self, name: str, region: str = "default"): ...
    def insert_arc(
        self,
        place: str,
        transition: str,
        variant: ArcVariant,
        name: str = "",
    ): ...

class ETCDGate:
    def __init__(
        self,
        endpoints,
        prefix="",
        node_name="default_node",
        region="default",
        lease_id=None,
        lease_ttl=timedelta(seconds=10),
        username=None,
        password=None,
    ): ...
