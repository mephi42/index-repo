extern crate index_repo;

#[cfg(test)]
mod test {
    use failure::Error;

    use index_repo::repomd;

    #[test]
    fn parse_repomd() -> Result<(), Error> {
        let doc = repomd::Document::parse(r#"<?xml version="1.0" encoding="UTF-8"?>
<repomd xmlns="http://linux.duke.edu/metadata/repo" xmlns:rpm="http://linux.duke.edu/metadata/rpm">
  <revision>1540419615</revision>
  <data type="primary">
    <checksum type="sha256">912f062d93e096c75901055ffca02a0c3961b33b8e1dd65319d97d493d3e49d5</checksum>
    <open-checksum type="sha256">e4007b1ed6155e2ab087f0afe99ceb859592221a6e66240a3895fa11483c7bcb</open-checksum>
    <location href="repodata/912f062d93e096c75901055ffca02a0c3961b33b8e1dd65319d97d493d3e49d5-primary.xml.gz"/>
    <timestamp>1540419511</timestamp>
    <size>16191653</size>
    <open-size>146578673</open-size>
  </data>
</repomd>
"#.as_bytes())?;
        assert_eq!(doc, repomd::Document {
            revision: 1540419615,
            data: vec![
                repomd::Data {
                    tpe: "primary".to_string(),
                    checksum: repomd::Checksum {
                        tpe: "sha256".to_string(),
                        hexdigest: "912f062d93e096c75901055ffca02a0c3961b33b8e1dd65319d97d493d3e49d5".to_string(),
                    },
                    open_checksum: Some(repomd::Checksum {
                        tpe: "sha256".to_string(),
                        hexdigest: "e4007b1ed6155e2ab087f0afe99ceb859592221a6e66240a3895fa11483c7bcb".to_string(),
                    }),
                    location: repomd::Location {
                        href: "repodata/912f062d93e096c75901055ffca02a0c3961b33b8e1dd65319d97d493d3e49d5-primary.xml.gz".to_string(),
                    },
                    timestamp: 1540419511,
                    size: 16191653,
                    open_size: Some(146578673),
                },
            ],
        });
        Ok(())
    }
}
