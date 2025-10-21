use scp::JsonRpcCx;
use tokio::process::Child;

pub struct Component {
    pub child: Child,
    pub jsonrpccx: JsonRpcCx,
}
