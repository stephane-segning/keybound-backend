use openssl::bn::BigNum;
use openssl::ec::{EcGroup, EcKey};
use openssl::nid::Nid;
use openssl::ecdsa::EcdsaSig;
use openssl::pkey::PKey;
use openssl::sign::Verifier;
use openssl::hash::MessageDigest;

fn main() {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    println!("Group created");
}
