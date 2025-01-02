pub mod error;
pub mod etcd;
pub mod exec;
pub mod net;
pub mod runner;
mod transition_runner;

pub use etcd::{
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
