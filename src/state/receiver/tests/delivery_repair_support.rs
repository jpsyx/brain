fn repair_identity_candidates(
    job_id: ReceiverJobId,
    token: &str,
    response_kind: &str,
) -> [String; 8] {
    std::array::from_fn(|index| {
        let seed = if index == 0 {
            format!("{job_id}:{token}:{response_kind}")
        } else {
            format!("{job_id}:{token}:{response_kind}:repair:{index}")
        };
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, seed.as_bytes()).to_string()
    })
}
