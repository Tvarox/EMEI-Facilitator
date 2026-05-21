use alloy_sol_types::sol;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IEMEIMandate,
    "abi/IEMEIMandate.json"
);
