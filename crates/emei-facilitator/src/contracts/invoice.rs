use alloy_sol_types::sol;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IEMEIInvoice,
    "abi/IEMEIInvoice.json"
);
