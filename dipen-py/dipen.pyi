# Configure the rust logging from the 'tracing' package to be logged via loguru
from datetime import timedelta
from enum import Enum
from typing import Any, Type

class RustTracingToLoguru:
    """
    Captures events of the Rust 'tracing' library, which is used for logging on the Rust side, and
    forwards them to loguru.

    Recommended to initialize this object and call 'install' before calling any other functions of
    this library.

    Note: Even though one could set the log level to 'TRACE' and filter everything from the loguru
    side, it is not recommended, as any log output sent to loguru requires acquistion of the GIL.

    TODO: this currently only sends the log message and does not care about extra fields or tracing
    scopes.
    """
    def __init__(self) -> None: ...
    log_level: str
    "Set default log level (active for packages not defined via 'set_target_log_level')"
    def set_target_log_level(self, prefix: str, level: str) -> None:
        """
        Set log level for a rust package prefix
        """
    def get_target_log_level(self, prefix: str) -> str | None: ...
    def install(self) -> None:
        """
        Installs the tracing subscriber in rust. Can only be called once during the lifetime of the
        current process.
        """

class ExecutorRegistry:
    """
    Used to register python classes to run the transitions in the net.
    """
    def __init__(self): ...
    def register(self, transition_name: str, transition_class: Type[Any], data: Any):
        """
        Register the given class as executor for this transition
        """

def start(
    net: PetriNetBuilder, etcd_config: ETCDConfig, executors: ExecutorRegistry
) -> RunHandle:
    """
    Main entrypoint into the distributed petri net runner.

    :param net: the petri net to run. Will be filtered by the 'region' provided in 'etcd_config'
    :param etcd_config: Configuration on how to connect to etcd
    :param executors: The transition executors defined and registered for the region.
                      Every transition needs an executor.
    :return: a RunHandle that allows to cancel / join on the runner.
    """

class RunHandle:
    def cancel(self) -> None:
        """
        Cancel the current run. Will give tasks one second before shutting everything down.
        """
    def join(self) -> None:
        """
        Blocks until the petri net runner has finished.
        """

    async def join_async(self) -> None:
        """
        Wait asynchronously until the petri net runner has finished.
        """

class ArcVariant(Enum):
    """
    Possible variants for an arc. A transition can
     - take tokens from any place connected with an 'In' arc.
     - decide whether to run, based on the tokens on places connected with 'Cond' or 'In' arcs.
     - place taken tokens on any place connected with an 'Out' arc, after running.

    There can only ever be one arc between a transition and a place.
    """

    In = ...
    Out = ...
    InOut = ...
    Cond = ...
    OutCond = ...

class PetriNetBuilder:
    def __init__(self) -> None: ...
    def insert_place(self, name: str, output_locking: bool = True):
        """
        Add a place to the net.

        :param output_locking: If false, the place will not be locked, before a finished transition
                               places a token on this place. Requires that the transitions that use
                               this place as a condition, do not care about additional tokens.
                               Typically used for a queue like place, where transitions always
                               takes the oldest token from the place.
        """
    def insert_transition(self, name: str, region: str = "default"):
        """
        Add a transition to the net.

        :param region: decides which runner executes this transition. One runner only ever runs a
                       single region
        """
    def insert_arc(
        self,
        place: str,
        transition: str,
        variant: ArcVariant,
        name: str = "",
    ):
        """
        Add an arc to the net.

        Place and transition must exist.
        """

class ETCDConfig:
    def __init__(
        self,
        endpoints: list[str],
        prefix: str = "",
        node_name: str = "default_node",
        region: str = "default",
        lease_id: int | None = None,
        lease_ttl: timedelta = timedelta(seconds=10),
        username: str | None = None,
        password: str | None = None,
    ):
        """
        Configure the connection to etcd
        :param prefix: Put in front of ALL etcd keys we read or write.
                       Mutltiple different petri nets could be run on one etcd cluster as long as
                       the prefixes differ and the total load is not too much for the cluster.
        :param lease_id: If provided, it must be a unique id for this client across the whole etcd
                         cluster, the prefix is irrelevant for leases. If not provided, a random one
                         is assigned.
        """

class ValidateContext:
    """
    Provided to the 'validate' function of your transition executor.
    """

    transition_name: str
    arcs: list[CreateArcContext]
    arcs_in: list[CreateArcContext]
    arcs_out: list[CreateArcContext]
    arcs_cond: list[CreateArcContext]
    def arcs_by_name(self, arc_name: str) -> list[CreateArcContext]: ...
    def arcs_by_place_name(self, place_name: str) -> list[CreateArcContext]: ...

class ValidateArcContext:
    arc_name: str
    variant: ArcVariant
    place_context: CreatePlaceContext

class ValidatePlaceContext:
    place_name: str

class ValidationResult:
    @staticmethod
    def succeeded() -> ValidationResult: ...
    @staticmethod
    def failed(reason: str) -> ValidationResult: ...

class CreateContext:
    """
    Provided to the '__init__' function of your transition executor.
    """

    transition_name: str
    transition_id: int
    arcs: list[CreateArcContext]
    arcs_in: list[CreateArcContext]
    arcs_out: list[CreateArcContext]
    arcs_cond: list[CreateArcContext]
    def arcs_by_name(self, arc_name: str) -> list[CreateArcContext]: ...
    def arcs_by_place_name(self, place_name: str) -> list[CreateArcContext]: ...

class CreateArcContext:
    arc_name: str
    variant: ArcVariant
    place_context: CreatePlaceContext

class CreatePlaceContext:
    place_name: str
    place_id: int

class StartContext:
    """
    Provided to the 'check_start' function of your transition executor.
    """

    tokens: list[StartTokenContext]
    taken_tokens: list[StartTakenTokenContext]
    def tokens_at(self, place_id: int) -> list[StartTokenContext]: ...
    def taken_tokens_at(self, place_id: int) -> list[StartTakenTokenContext]: ...

class StartTokenContext:
    token_id: int
    place_id: int
    data: bytes

class StartTakenTokenContext:
    token_id: int
    place_id: int
    transition_id: int
    data: bytes

# Must be returned from the 'check_start'  function of a transition
class CheckStartResult:
    """
    Must be returned from the 'check_start' function.
    """
    @staticmethod
    def build() -> CheckStartResultBuilder: ...

class CheckStartResultBuilder:
    def take(self, token: StartTokenContext):
        """
        When firing, take this token.

        This does not directly change any state. But if the runner decides to fire this transition
        based on the result of the 'check_start' function, the tokens given to 'take' are taken
        into the transition and are available from the RunContext.
        """
    def enabled(self) -> CheckStartResult:
        """
        Creates an 'enabled' CheckStartResult return value, runner will try to fire the transition.
        Note that the 'check_start' function might be called again before actually firing, even if
        'enabled' is returned.

        Current implementation: 'check_start' is always called twice. The first time with the
        current local state. If the result is 'enabled', the input / conditional places are locked
        and it is called with the locked state again. The state might be different. If that still
        returns 'enabled', the transition is fired.
        """
    def disabled(
        self, wait_for: int | None = None, auto_recheck: timedelta | None = None
    ):
        """
        Creates a 'disabled' CheckStartResult return value, runner will not fire the transition.

        :param wait_for: Only check the given place id for changes. Otherwise any change in any of
                         the connected input / conditional places will trigger the 'check_start'
                         function again.
        :param auto_recheck: If provided, the runner will recheck this transition automatically
                             after the given time, even without any changes in the input /
                             conditional places
        """

class RunContext:
    """
    Provided to the async 'run' function of your transition executor.
    """

    tokens: list[RunTokenContext]

class RunTokenContext:
    token_id: int
    data: bytes
    orig_place_id: int

# Has to be returned from the 'run' method of a transition
class RunResult:
    """
    Must be returned from the async 'run' function.
    """
    @staticmethod
    def build() -> RunResultBuilder: ...

class RunResultBuilder:
    def place(self, token: RunTokenContext, place_id: int):
        """
        Upon finishing the current run, place this token on the given place.
        """
    def update(self, token: RunTokenContext, data: bytes):
        """
        Upon finishing the current run, update the data of this token.
        """
    def place_new(self, place_id: int, data: bytes):
        """
        Upon finishing the current run, create a new token with the given data on the given place.
        """
    def result(self) -> RunResult:
        """
        Create a RunResult to return from the 'run' function.

        Note that all tokens that have been taken (all tokens in RunContext.tokens) that have not
        been placed somewhere will be deleted.
        """
