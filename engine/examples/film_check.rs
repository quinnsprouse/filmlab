use filmlab_engine::film::{FilmChain, FilmRequest, LutLibrary};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, negative, print, colors] = args.as_slice() else {
        return Err("usage: film_check <negative.cube> <print.cube> <colors.json (rec709 linear)>".into());
    };
    let mut luts = LutLibrary::default();
    luts.insert("n", &std::fs::read_to_string(negative).map_err(|e| e.to_string())?)?;
    luts.insert("p", &std::fs::read_to_string(print).map_err(|e| e.to_string())?)?;
    let request = FilmRequest { negative: Some("n".into()), print: Some("p".into()), ..Default::default() };
    let chain = FilmChain::new(&luts, &request)?.ok_or("no chain")?;
    let colors: Vec<[f32; 3]> = serde_json::from_str(&std::fs::read_to_string(colors).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let m = [[0.627_404, 0.329_283, 0.043_313], [0.069_097, 0.919_540, 0.011_362], [0.016_391, 0.088_013, 0.895_595]];
    let out: Vec<[f32; 3]> = colors
        .iter()
        .map(|c| {
            let c2020 = [0, 1, 2].map(|i| m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]);
            chain.print_display(chain.negative_of(chain.log_of(c2020)))
        })
        .collect();
    println!("{}", serde_json::to_string(&out).map_err(|e| e.to_string())?);
    Ok(())
}
