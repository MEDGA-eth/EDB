use once_cell::sync::Lazy;
use rand::seq::SliceRandom;
use std::sync::atomic::{AtomicUsize, Ordering};

// List of etherscan keys for mainnet
static ETHERSCAN_MAINNET_KEYS: Lazy<Vec<&'static str>> = Lazy::new(|| {
    let mut keys = vec![
        "MCAUM7WPE9XP5UQMZPCKIBUJHPM1C24FP6",
        "JW6RWCG2C5QF8TANH4KC7AYIF1CX7RB5D1",
        "ZSMDY6BI2H55MBE3G9CUUQT4XYUDBB6ZSK",
        "4FYHTY429IXYMJNS4TITKDMUKW5QRYDX61",
        "QYKNT5RHASZ7PGQE68FNQWH99IXVTVVD2I",
        "VXMQ117UN58Y4RHWUB8K1UGCEA7UQEWK55",
        "C7I2G4JTA5EPYS42Z8IZFEIMQNI5GXIJEV",
        "A15KZUMZXXCK1P25Y1VP1WGIVBBHIZDS74",
        "3IA6ASNQXN8WKN7PNFX7T72S9YG56X9FPG",
    ];

    keys.shuffle(&mut rand::thread_rng());

    keys
});

// counts the next etherscan key to use
static NEXT_ETHERSCAN_MAINNET_KEY: AtomicUsize = AtomicUsize::new(0);

// returns the current value of the atomic counter and increments it
fn next(c: &AtomicUsize) -> usize {
    c.fetch_add(1, Ordering::SeqCst)
}

pub fn next_etherscan_api_key() -> String {
    let idx = next(&NEXT_ETHERSCAN_MAINNET_KEY) % ETHERSCAN_MAINNET_KEYS.len();
    ETHERSCAN_MAINNET_KEYS[idx].to_string()
}
