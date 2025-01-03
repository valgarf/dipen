import sys

from loguru import logger

import dipen

LOGURU_FORMAT = (
    "<green>{time:YYYY-MM-DD HH:mm:ss.SSS}</green> | "
    "<level>{level: <8}</level> | "
    "<cyan>{file.name}</cyan>:<cyan>{line}</cyan> - <level>{message}</level>"
)


def main():
    logger.remove()
    logger.add(sys.stdout, format=LOGURU_FORMAT, level="DEBUG")
    rust_logging = dipen.RustTracingToLoguru()
    rust_logging.log_level = "info"
    rust_logging.set_target_log_level("dipen", "debug")
    rust_logging.install()

    net = dipen.PetriNetBuilder()

    net.insert_place("pl1", True)
    net.insert_place("pl2", True)
    net.insert_transition("tr1", "region-1")
    net.insert_transition("tr2", "region-1")
    net.insert_arc("pl1", "tr1", dipen.ArcVariant.In)
    net.insert_arc("pl2", "tr1", dipen.ArcVariant.Out)
    net.insert_arc("pl2", "tr2", dipen.ArcVariant.In)
    net.insert_arc("pl1", "tr2", dipen.ArcVariant.Out)
    net.insert_transition("tr-init", "region-1")
    net.insert_arc("pl1", "tr-init", dipen.ArcVariant.OutCond)
    net.insert_arc("pl2", "tr-init", dipen.ArcVariant.Cond)

    dipen.run(net)  # type: ignore


if __name__ == "__main__":
    main()
