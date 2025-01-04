# DiPeN - distributed petri net runner

This library implements a distributed petri net runner with the goal to be usable as a workflow 
engine, that is, driving real workflows, not just simulating them.

The petri net differs from the classical petri net in multiple areas:
- Tokens are colored: Tokens can have arbitrary data attached to them. 
- Transitions are timed: You are expected to write asynchronous code to be executed when a transition is firing.
- Transition have arbitrary guard expressions: You can write any logic you want to decide whether a transition can be fired.

DiPeN takes care of synchronizing the state of the petri net between different runners. 
It uses [etcd](https://etcd.io/) to do so. 
Different runners must be responsible for different regions (groups of transitions) if they should run in parallel.
Failover runners are supported: Multiple runners for the same region can be started, but only one will do any work. As soon as the leader dies (or is disconnected from etcd) another runner takes over. 

The library is written in Rust but has Python bindings available.

## Getting Started

You need a running etcd installation. 
I recommend starting a [docker container](https://etcd.io/docs/v3.5/op-guide/container/), 
for other installation options, see [etcd docs](https://etcd.io/docs/v3.5/install/)

### Rust

You can install DiPeN from [crates.io](https://crates.io/). See dipen/examples in the repository to get started.

### Python

You can install DiPeN from [pypi](https://pypi.org/). See dipen-py/examples in the repositry to get started.

## ToDo

This library is currently a proof of concept. For it to be usable in a productions setting, the 
better part of the following list of features should be implemented (and probably more stuff I have not thought about yet):

- improve python bindings
    * error handling could be improved (what should a transition do if the python run function fails?)
    * improve logging to  include fields from scopes, log scope start / end
    * registry data not passed through to validate / create
    * pass cancellation through to running python transitions
- tests for this package
- simplify testing for users
    * implement a 'local' runner for testing (extract etcd logic into traits)
    * snapshottest, i.e. replay run based on a stored run, allow storing the run (special runner?)
    * export (parts of) the etcd history to be used for tests
- Visualization 
    * live view of current state
      - mark running transitions? currently not stored on etcd
      - human readable reasoning for not firing?
    * history from etcd?
    * export of petri net as graph
- documentation
    * python & rust are both rudimentary
    * more examples 
      -> need ideas for a nontrivial but not too complex workflow
      -> simulating an intersection?
- time data: etcd has no synchronized clock in any way
    * store 'last_changed', 'last_start_firing', 'last_finished_firing', etc. time from local time?
      -> startup time for initial load? 
    * store local time of runners on etcd with every update? Could be used in visualization to show
      an estimated time of the last change
- migration strategies / manual interaction:
    * remove transitions from own region on etcd, if not in current configuration?
      -> 'transfer' of transition region: first update runner that does not use the transition anymore, then update runner for the new region
    * allow manual blocking of regions / transitions (will not start firing)
    * allow manual cancellation of regions / transitins (cancel currently running)
    * prepared 'manual transitions' that only fire on manual input (with additional provided data)?
    * 'ad hoc' manual transtions that take / place from / to arbitrary places with arbitrary updates to modify state?
- statistics (transitions / s, how often was each transition fired)? 

 
## License

MIT