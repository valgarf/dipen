import asyncio
import sys

from loguru import logger

import dipen

LOGURU_FORMAT = (
    "<green>{time:YYYY-MM-DD HH:mm:ss.SSS}</green> | "
    "<level>{level: <8}</level> | "
    "<cyan>{file.name}</cyan>:<cyan>{line}</cyan> - <level>{message}</level>"
)


class TrInitialize:
    @staticmethod
    def validate(ctx):
        return True

    def __init__(self, ctx):
        self.pl_out = ctx.arcs_out[0].place_context.place_id
        self.pl_ids = [a.place_context.place_id for a in ctx.arcs_cond]

    def check_start(self, ctx):
        count = sum(
            len(ctx.tokens_at(pl_id)) + len(ctx.taken_tokens_at(pl_id))
            for pl_id in self.pl_ids
        )
        if count > 0:
            return dipen.CheckStartResult.build().disabled()
        return dipen.CheckStartResult.build().enabled()

    async def run(self, ctx):
        await asyncio.sleep(1)
        result = dipen.RunResult.build()
        result.place_new(self.pl_out, "newly created".encode())
        return result.result()


class TrDelayedMove:
    @staticmethod
    def validate(ctx):
        return True

    def __init__(self, ctx):
        self.pl_in = ctx.arcs_in[0].place_context.place_id
        self.pl_out = ctx.arcs_out[0].place_context.place_id
        self.tr_name = ctx.transition_name

    def check_start(self, ctx):
        tokens_in = ctx.tokens_at(self.pl_in)
        if not tokens_in:
            return dipen.CheckStartResult.build().disabled()
        result = dipen.CheckStartResult.build()
        result.take(tokens_in[0])
        return result.enabled()

    async def run(self, ctx):
        await asyncio.sleep(1)
        result = dipen.RunResult.build()
        for to in ctx.tokens:
            result.place(to, self.pl_out)
            result.update(
                to, f"Placed from python by transition {self.tr_name}".encode()
            )
        return result.result()


async def async_main(net, etcd, executors):
    handle = dipen.start(net, etcd, executors)
    await asyncio.sleep(5)
    handle.cancel()
    return handle


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

    executors = dipen.ExecutorRegistry()
    executors.register("tr1", TrDelayedMove, None)
    executors.register("tr2", TrDelayedMove, None)
    executors.register("tr-init", TrInitialize, None)

    etcd = dipen.ETCDGate(
        endpoints=["localhost:2379"],
        prefix="py-01-endless-loop/",
        node_name="node1",
        region="region-1",
    )
    handle = asyncio.run(async_main(net, etcd, executors))
    handle.join()


if __name__ == "__main__":
    main()