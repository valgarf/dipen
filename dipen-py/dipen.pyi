# Configure the rust logging from the 'tracing' package to be logged via loguru
class RustTracingToLoguru:
    def __init__(self) -> None: ...
    # Set default log level (active for packages not defined via 'set_target_log_level')
    log_level: str
    # Set log level for a rust package prefix
    def set_target_log_level(self, prefix: str, level: str) -> None: ...
    # Installs the tracing subscriber. Can only be called once during the lifetime of the current
    # process
    def install(self) -> None: ...
