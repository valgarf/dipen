pub mod error;
mod etcd_gate;
pub mod net;
pub mod runner;
pub mod transition;
mod transition_runner;

pub use etcd_gate::{
    ETCDConfig, ETCDConfigBuilder, ETCDConfigBuilderError, ETCDGate, ETCDTransitionGate,
};

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
