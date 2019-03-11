CREATE TABLE elf_symbols_tmp
(
  id       INTEGER NOT NULL PRIMARY KEY,
  file_id  INTEGER NOT NULL,
  name_id  INTEGER NOT NULL,
  st_info  INTEGER NOT NULL,
  st_other INTEGER NOT NULL,
  FOREIGN KEY (file_id) REFERENCES files (id),
  FOREIGN KEY (name_id) REFERENCES strings (id)
);
INSERT INTO elf_symbols_tmp
SELECT id, file_id, name_id, st_info, st_other
FROM elf_symbols;
DROP TABLE elf_symbols;
ALTER TABLE elf_symbols_tmp
  RENAME TO elf_symbols;
CREATE INDEX elf_symbols_name_id_index ON elf_symbols (name_id);
