use alloy_sol_types::sol;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IMockERC8004,
    "abi/IMockERC8004.json"
);
