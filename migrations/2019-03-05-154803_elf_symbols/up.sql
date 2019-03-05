CREATE TABLE packages
(
  id      INTEGER NOT NULL PRIMARY KEY,
  repo_id INTEGER NOT NULL,
  name    VARCHAR NOT NULL,
  arch    VARCHAR NOT NULL,
  version VARCHAR NOT NULL,
  epoch   VARCHAR NOT NULL,
  release VARCHAR NOT NULL,
  FOREIGN KEY (repo_id) REFERENCES repos (id)
);
CREATE TABLE files
(
  id         INTEGER NOT NULL PRIMARY KEY,
  name       VARCHAR NOT NULL,
  package_id INTEGER NOT NULL,
  FOREIGN KEY (package_id) REFERENCES packages (id)
);
CREATE TABLE strings
(
  id   INTEGER NOT NULL PRIMARY KEY,
  name VARCHAR NOT NULL
);
CREATE TABLE elf_symbols
(
  id       INTEGER NOT NULL PRIMARY KEY,
  file_id  INTEGER NOT NULL,
  name_id  VARCHAR NOT NULL,
  st_info  INTEGER NOT NULL,
  st_other INTEGER NOT NULL,
  FOREIGN KEY (file_id) REFERENCES files (id),
  FOREIGN KEY (name_id) REFERENCES strings (id)
);
