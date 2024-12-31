use byteorder::{ReadBytesExt, WriteBytesExt, LE};
use petri_etcd_runner::{
    net::{PlaceId, TransitionId},
    transition::{
        CheckStartResult, CreateArcContext, CreatePlaceContext, RunResult, RunTokenContext,
        StartTokenContext, TransitionExecutor, ValidationResult,
    },
};

pub struct Move {
    pl_in: PlaceId,
    pl_out: PlaceId,
    tr_id: TransitionId,
    tr_name: String,
}

impl TransitionExecutor for Move {
    fn validate(ctx: &impl petri_etcd_runner::transition::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
        if ctx.arcs_in().count() == 1 && ctx.arcs_out().count() == 1 {
            ValidationResult::success()
        } else {
            ValidationResult::failure("Need exactly one incoming and one outgoing arc")
        }
    }

    fn new(ctx: &impl petri_etcd_runner::transition::CreateContext) -> Self
    where
        Self: Sized,
    {
        let pl_in = ctx.arcs_in().next().unwrap().place_context().place_id();
        let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
        let tr_name: String = ctx.transition_name().into();

        Move { pl_in, pl_out, tr_id: ctx.transition_id(), tr_name }
    }

    fn check_start(
        &mut self,
        ctx: &mut impl petri_etcd_runner::transition::StartContext,
    ) -> CheckStartResult {
        let next_token = ctx.tokens_at(self.pl_in).next();
        let mut result = CheckStartResult::build();
        match next_token {
            Some(to) => {
                if to.data().read_u16::<LE>().expect("failed to read number") == 0 {
                    result.disabled(Some(self.pl_in), None)
                } else {
                    result.take(&to);
                    result.enabled()
                }
            }
            None => result.disabled(Some(self.pl_in), None),
        }
    }

    async fn run(&mut self, ctx: &mut impl petri_etcd_runner::transition::RunContext) -> RunResult {
        let mut result = RunResult::build();
        for to in ctx.tokens() {
            result.place(to, self.pl_out);
            let num = to.data().read_u16::<LE>().expect("failed to read number");
            let mut result_data = vec![];
            result_data.write_u16::<LE>(num - 1).expect("failed to write number");

            result.update(to, result_data);
        }
        result.result()
    }
}
