use ramseynet_graph::{compute_cid, rgxf};
use ramseynet_verifier::{verify_ramsey, VerifyRequest, VerifyResponse};

/// OVWC-1 binary entry point.
/// Reads a VerifyRequest JSON from stdin, writes a VerifyResponse JSON to stdout.
fn main() {
    let request: VerifyRequest = match serde_json::from_reader(std::io::stdin()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to parse VerifyRequest: {e}");
            std::process::exit(1);
        }
    };

    let adj = match rgxf::from_json(&request.graph) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to decode RGXF: {e}");
            std::process::exit(1);
        }
    };

    let cid = compute_cid(&adj);
    let result = verify_ramsey(&adj, request.k, request.ell, &cid);

    let mut response: VerifyResponse = result.into();
    if !request.want_cid {
        response.graph_cid = None;
    }

    serde_json::to_writer(std::io::stdout(), &response).unwrap();
}
