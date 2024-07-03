#[macro_export]
macro_rules! init_progress {
    ($local:expr, $label:expr) => {{
        pub fn eta_key(state: &indicatif::ProgressState, f: &mut dyn std::fmt::Write) {
            write!(f, "{:.1}s", state.eta().as_secs_f64()).unwrap()
        }

        let pb = indicatif::ProgressBar::new($local.len() as u64);
        let mut template =
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ".to_string();
        template += $label;
        template += " ({eta})";
        pb.set_style(
            indicatif::ProgressStyle::with_template(&template)
                .unwrap()
                .with_key("eta", eta_key)
                .progress_chars("#>-"),
        );
        pb.set_position(0);
        pb
    }};
}

#[macro_export]
macro_rules! update_progress {
    ($pb:ident, $index:expr) => {
        $pb.set_position(($index + 1) as u64);
    };
}
