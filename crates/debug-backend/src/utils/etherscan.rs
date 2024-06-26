/// Automaticall pause the request if the rate limit is reached
/// and resume it after the rate limit is reset.
#[macro_export]
macro_rules! etherscan_rate_limit_guard {
    ($request:expr) => {
        loop {
            match $request {
                Ok(response) => break Ok(response),
                Err(foundry_block_explorers::errors::EtherscanError::RateLimitExceeded) => {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue
                }
                Err(e) => break Err(e),
            }
        }
    };

    ($request:expr, $secs:expr) => {
        loop {
            match $request {
                Ok(response) => break Ok(response),
                Err(foundry_block_explorers::errors::EtherscanError::RateLimitExceeded) => {
                    tokio::time::sleep(tokio::time::Duration::from_secs($secs)).await;
                    continue
                }
                Err(e) => break Err(e),
            }
        }
    };
}
