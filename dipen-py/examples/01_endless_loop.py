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
    dipen.run()  # type: ignore


if __name__ == "__main__":
    main()
