pub mod opcode;

#[macro_export]
macro_rules! vec_to_vars {
    ($vec:expr, $($var:ident),+) => {
        let [$($var),+] = match $vec.as_slice() {
            &[$($var),+] => [$($var),+],
            _ => panic!("Vector size does not match the number of variables"),
        };
    };
}
