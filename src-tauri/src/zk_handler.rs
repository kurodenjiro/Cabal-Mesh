use serde::{Deserialize, Serialize};
use std::error::Error;
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct ZKProof {
    pub proof: String,
    pub public_inputs: Vec<String>,
    pub encrypted_intent: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProofRequest {
    pub balance: u64,        // private
    pub bid_amount: u64,     // public
    pub price_ceiling: u64,  // private
}

pub struct ZKHandler {
    circuit_path: String,
}

impl ZKHandler {
    pub fn new(circuit_path: Option<String>) -> Self {
        ZKHandler {
            circuit_path: circuit_path.unwrap_or_else(|| "./noir-circuit".to_string()),
        }
    }

    /// Generate a zero-knowledge proof that:
    /// 1. balance >= bid_amount
    /// 2. bid_amount <= price_ceiling
    pub async fn generate_proof(&self, request: ProofRequest) -> Result<ZKProof, Box<dyn Error>> {
        println!("🔐 Generating Noir ZK-Proof...");
        println!("   Balance (private): {}", request.balance);
        println!("   Bid Amount (public): {}", request.bid_amount);
        println!("   Price Ceiling (private): {}", request.price_ceiling);

        // Verify locally before generating proof
        if request.balance < request.bid_amount {
            return Err("Insufficient balance".into());
        }

        if request.bid_amount > request.price_ceiling {
            return Err("Bid exceeds price ceiling".into());
        }

        // Execute Noir build command in blocking task to avoid freezing async runtime
        // Note: This requires nargo (Noir compiler) to be installed
        let circuit_path = self.circuit_path.clone();
        
        let output_result = tokio::task::spawn_blocking(move || {
            Command::new("nargo")
                .args(&["prove", &circuit_path])
                .output()
        }).await?;

        match output_result {
            Ok(result) if result.status.success() => {
                // Read the generated proof
                let proof_data = std::fs::read_to_string(format!("{}/proofs/proof.hex", self.circuit_path))?;

                Ok(ZKProof {
                    proof: proof_data,
                    public_inputs: vec![request.bid_amount.to_string()],
                    encrypted_intent: format!(
                        "{{\"bid\":{},\"verified\":true}}",
                        request.bid_amount
                    ),
                })
            }
            _ => {

                let error_msg = "❌ Noir ZK Proof Generation Failed! Ensure 'nargo' is installed and circuit is compiled.";
                eprintln!("{}", error_msg);
                Err(error_msg.into())
            }
            }
        }




    pub fn verify_proof(&self, proof: &ZKProof) -> Result<bool, Box<dyn Error>> {
        // In production, this would call Noir's verification
        // For now, we validate the structure
        Ok(!proof.proof.is_empty() && !proof.public_inputs.is_empty())
    }
}

// Noir circuit template (to be saved as circuits/main.nr)
pub const NOIR_CIRCUIT_TEMPLATE: &str = r#"
// CabalMesh - Privacy-Preserving Bid Verification Circuit
// This proves: balance >= bid_amount AND bid_amount <= price_ceiling

fn main(
    balance: Field,        // private witness
    bid_amount: pub Field, // public input
    price_ceiling: Field   // private witness
) {
    // Constraint 1: User has sufficient balance
    assert(balance >= bid_amount);
    
    // Constraint 2: Bid doesn't exceed the user's private limit
    assert(bid_amount <= price_ceiling);
    
    // Additional constraint: Bid must be positive
    assert(bid_amount > 0);
}
"#;
