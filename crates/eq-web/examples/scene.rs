//! Prints the scene JSON after playing the lines given as arguments.
//!
//! ```sh
//! cargo run -p eq-web --example scene -- north gather gather
//! ```
//!
//! Handy for seeing exactly what the browser is given to draw.

use eq_web::Shore;

fn main() {
    let mut shore = Shore::new(1);
    for line in std::env::args().skip(1) {
        shore.input(&line);
    }
    println!("{}", shore.scene());
}
