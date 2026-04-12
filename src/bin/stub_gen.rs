fn main() -> pyo3_stub_gen::Result<()> {
    let stub = roller::stub_info()?;
    stub.generate()?;
    Ok(())
}
