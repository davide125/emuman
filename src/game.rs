use super::Error;
use serde_derive::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GameDb {
    pub description: String,
    pub games: HashMap<String, Game>,
}

impl GameDb {
    #[inline]
    pub fn is_game(&self, game: &str) -> bool {
        self.games.contains_key(game)
    }

    #[inline]
    pub fn all_games(&self) -> Vec<String> {
        self.games.keys().cloned().collect()
    }

    pub fn validate_games<I>(&self, games: I) -> Result<(), Error>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        games.into_iter().try_for_each(|s| {
            if self.is_game(s.as_ref()) {
                Ok(())
            } else {
                Err(Error::NoSuchSoftware(s.as_ref().to_string()))
            }
        })
    }

    pub fn required_parts<I>(&self, games: I) -> Result<HashSet<Part>, Error>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let mut parts = HashSet::new();
        for game in games.into_iter() {
            if let Some(game) = self.games.get(game.as_ref()) {
                parts.extend(game.parts.values().cloned());
            } else {
                return Err(Error::NoSuchSoftware(game.as_ref().to_string()));
            }
        }
        Ok(parts)
    }

    pub fn verify(&self, root: &Path, games: &HashSet<String>) -> usize {
        use rayon::prelude::*;

        games
            .par_iter()
            .map(|game| {
                let failures = self.verify_game(root, game);
                if failures.is_empty() {
                    println!("{} : OK", game);
                    1
                } else {
                    use std::io::{stdout, Write};

                    // ensure results are generated as a unit
                    let stdout = stdout();
                    let mut handle = stdout.lock();
                    writeln!(&mut handle, "{} : BAD", game).unwrap();
                    for failure in failures {
                        writeln!(&mut handle, "  {}", failure).unwrap();
                    }
                    0
                }
            })
            .sum()
    }

    fn verify_game(&self, root: &Path, game_name: &str) -> Vec<VerifyFailure<PathBuf>> {
        if let Some(game) = self.games.get(game_name) {
            let mut results = game.verify(&root.join(game_name));
            results.extend(
                game.devices
                    .iter()
                    .flat_map(|device| self.verify_game(root, device)),
            );
            results
        } else {
            Vec::new()
        }
    }

    pub fn list(&self, search: Option<&str>, sort: SortBy, simple: bool) {
        let mut results: Vec<&Game> = if let Some(search) = search {
            self.games
                .values()
                .filter(|g| !g.is_device && g.matches(search))
                .collect()
        } else {
            self.games.values().filter(|g| !g.is_device).collect()
        };

        Game::sort_report(&mut results, sort);
        GameDb::display_report(&results, simple)
    }

    pub fn games<I>(&self, games: I, simple: bool)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        GameDb::display_report(
            &games
                .into_iter()
                .filter_map(|g| self.games.get(g.as_ref()).filter(|g| !g.is_device))
                .collect::<Vec<&Game>>(),
            simple,
        )
    }

    pub fn report(
        &self,
        games: &HashSet<String>,
        search: Option<&str>,
        sort: SortBy,
        simple: bool,
    ) {
        let mut results: Vec<&Game> = games
            .iter()
            .filter_map(|g| self.games.get(g).filter(|g| !g.is_device))
            .collect();
        if let Some(search) = search {
            results.retain(|g| g.matches(search));
        }

        Game::sort_report(&mut results, sort);
        GameDb::display_report(&results, simple)
    }

    fn display_report(games: &[&Game], simple: bool) {
        use prettytable::{cell, format, row, Table};

        #[inline]
        pub fn no_parens(s: &str) -> &str {
            if let Some(index) = s.find('(') {
                s[0..index].trim_end()
            } else {
                s
            }
        }

        #[inline]
        pub fn no_slashes(s: &str) -> &str {
            if let Some(index) = s.find(" / ") {
                s[0..index].trim_end()
            } else {
                s
            }
        }

        let mut table = Table::new();
        table.set_format(*format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR);
        for game in games {
            let description = if simple {
                no_slashes(no_parens(&game.description))
            } else {
                &game.description
            };
            let creator = if simple {
                no_parens(&game.creator)
            } else {
                &game.creator
            };
            let year = &game.year;
            let name = &game.name;

            table.add_row(match game.status {
                Status::Working => row![description, creator, year, name],
                Status::Partial => row![FY => description, creator, year, name],
                Status::NotWorking => row![FR => description, creator, year, name],
            });
        }

        table.printstd();
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Status {
    Working,
    Partial,
    NotWorking,
}

impl Default for Status {
    fn default() -> Self {
        Status::Working
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Game {
    pub name: String,
    pub description: String,
    pub creator: String,
    pub year: String,
    pub status: Status,
    pub is_device: bool,
    pub parts: HashMap<String, Part>,
    pub devices: Vec<String>,
}

impl Game {
    pub fn matches(&self, search: &str) -> bool {
        self.name.starts_with(search)
            || self.description.contains(search)
            || self.creator.contains(search)
            || (self.year == search)
    }

    pub fn sort_key(&self, sort: SortBy) -> &str {
        match sort {
            SortBy::Description => &self.description,
            SortBy::Creator => &self.creator,
            SortBy::Year => &self.year,
        }
    }

    pub fn sort_report(games: &mut Vec<&Game>, sort: SortBy) {
        match sort {
            SortBy::Description => {
                games.sort_unstable_by_key(|x| x.sort_key(SortBy::Year));
                games.sort_by_key(|x| x.sort_key(SortBy::Creator));
                games.sort_by_key(|x| x.sort_key(SortBy::Description));
            }
            SortBy::Creator => {
                games.sort_unstable_by_key(|x| x.sort_key(SortBy::Year));
                games.sort_by_key(|x| x.sort_key(SortBy::Description));
                games.sort_by_key(|x| x.sort_key(SortBy::Creator));
            }
            SortBy::Year => {
                games.sort_unstable_by_key(|x| x.sort_key(SortBy::Creator));
                games.sort_by_key(|x| x.sort_key(SortBy::Description));
                games.sort_by_key(|x| x.sort_key(SortBy::Year));
            }
        }
    }

    fn verify(&self, game_root: &Path) -> Vec<VerifyFailure<PathBuf>> {
        use rayon::prelude::*;
        use std::fs::read_dir;
        use std::sync::Mutex;

        let mut failures = Vec::new();

        // turn files on disk into a map, so extra files can be located
        let mut files_on_disk: HashMap<String, PathBuf> = HashMap::new();
        if let Ok(dir) = read_dir(game_root) {
            for entry in dir.filter_map(|e| e.ok()) {
                if let Ok(name) = entry.file_name().into_string() {
                    files_on_disk.insert(name, entry.path());
                } else {
                    // anything that's not UTF-8 is an extra
                    failures.push(VerifyFailure::Extra(entry.path()));
                }
            }
        }

        // verify all game parts
        let files_on_disk = Mutex::new(files_on_disk);
        failures.extend(
            self.parts
                .par_iter()
                .filter_map(|(name, part)| {
                    if let Some(pathbuf) = files_on_disk.lock().unwrap().remove(name) {
                        part.verify(pathbuf)
                    } else {
                        Some(VerifyFailure::Missing(game_root.join(name)))
                    }
                })
                .collect::<Vec<VerifyFailure<PathBuf>>>()
                .into_iter(),
        );

        // mark any leftover files on disk as extras
        failures.extend(
            files_on_disk
                .into_inner()
                .unwrap()
                .into_iter()
                .map(|(_, pathbuf)| VerifyFailure::Extra(pathbuf)),
        );

        failures
    }

    pub fn add(
        &self,
        rom_sources: &HashMap<Part, PathBuf>,
        target: &Path,
        copy: fn(&Part, &Path, &Path) -> Result<(), std::io::Error>,
    ) -> Result<(), Error> {
        let target_dir = target.join(&self.name);
        self.parts
            .iter()
            .try_for_each(|(rom_name, part)| {
                if let Some(source) = rom_sources.get(&part) {
                    copy(&part, source, &target_dir.join(rom_name))
                } else {
                    Ok(())
                }
            })
            .map_err(Error::IO)
    }
}

pub enum VerifyFailure<P> {
    Missing(P),
    Extra(P),
    Bad(P),
    Error(P, std::io::Error),
}

impl<P: AsRef<Path>> fmt::Display for VerifyFailure<P> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VerifyFailure::Missing(pb) => write!(f, "MISSING : {}", pb.as_ref().display()),
            VerifyFailure::Extra(pb) => write!(f, "EXTRA : {}", pb.as_ref().display()),
            VerifyFailure::Bad(pb) => write!(f, "BAD : {}", pb.as_ref().display()),
            VerifyFailure::Error(pb, err) => {
                write!(f, "ERROR : {} : {}", pb.as_ref().display(), err)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Part {
    ROM { sha1: String },
    Disk { sha1: String },
}

impl Part {
    #[inline]
    pub fn rom_from_sha1(sha1: String) -> Self {
        Part::ROM { sha1 }
    }

    #[inline]
    pub fn disk_from_sha1(sha1: String) -> Self {
        Part::Disk { sha1 }
    }

    #[inline]
    pub fn name_to_chd(name: &str) -> String {
        let mut d = name.to_string();
        d.push_str(".chd");
        d
    }

    pub fn from_path(path: &Path) -> Result<Self, std::io::Error> {
        use std::fs::File;
        use std::io::{BufReader, Seek, SeekFrom};

        let mut r = BufReader::new(File::open(path)?);

        match Part::disk_from_reader(&mut r) {
            Ok(Some(disk)) => Ok(disk),
            Ok(None) => {
                r.seek(SeekFrom::Start(0))?;
                Part::rom_from_reader(r)
            }
            Err(err) => Err(err),
        }
    }

    fn disk_from_reader<R: BufRead>(mut r: R) -> Result<Option<Self>, std::io::Error> {
        fn skip<R: BufRead>(mut r: R, mut to_skip: usize) -> Result<(), std::io::Error> {
            while to_skip > 0 {
                let consumed = r.fill_buf()?.len().min(to_skip);
                r.consume(consumed);
                to_skip -= consumed;
            }
            Ok(())
        }

        let mut tag = [0; 8];

        if r.read_exact(&mut tag).is_err() || &tag != b"MComprHD" {
            // non-CHD files might be less than 8 bytes
            return Ok(None);
        }

        // at this point we'll treat the file as a CHD

        skip(&mut r, 4)?; // unused length field

        let mut version = [0; 4];
        r.read_exact(&mut version)?;

        let bytes_to_skip = match u32::from_be_bytes(version) {
            3 => (32 + 32 + 32 + 64 + 64 + 8 * 16 + 8 * 16 + 32) / 8,
            4 => (32 + 32 + 32 + 64 + 64 + 32) / 8,
            5 => (32 * 4 + 64 + 64 + 64 + 32 + 32 + 8 * 20) / 8,
            _ => return Ok(None),
        };
        skip(&mut r, bytes_to_skip)?;

        let mut sha1 = [0; 20];
        r.read_exact(&mut sha1)?;
        Ok(Some(Part::Disk {
            sha1: sha1.iter().map(|b| format!("{:02x}", b)).collect(),
        }))
    }

    fn rom_from_reader<R: Read>(mut r: R) -> Result<Self, std::io::Error> {
        use sha1::Sha1;

        let mut sha1 = Sha1::new();
        let mut buf = [0; 4096];
        loop {
            match r.read(&mut buf) {
                Ok(0) => {
                    return Ok(Part::ROM {
                        sha1: sha1.hexdigest(),
                    })
                }
                Ok(bytes) => sha1.update(&buf[0..bytes]),
                Err(err) => return Err(err),
            }
        }
    }

    fn verify<P: AsRef<Path>>(&self, part_path: P) -> Option<VerifyFailure<P>> {
        match Part::from_path(part_path.as_ref()) {
            Ok(ref disk_part) if self == disk_part => None,
            Ok(_) => Some(VerifyFailure::Bad(part_path)),
            Err(err) => Some(VerifyFailure::Error(part_path, err)),
        }
    }
}

#[derive(Copy, Clone)]
pub enum SortBy {
    Description,
    Creator,
    Year,
}

impl FromStr for SortBy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "description" => Ok(SortBy::Description),
            "creator" => Ok(SortBy::Creator),
            "year" => Ok(SortBy::Year),
            _ => Err("invalid sort by value".to_string()),
        }
    }
}

fn subdir_files(root: &Path) -> Vec<PathBuf> {
    use walkdir::WalkDir;

    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| {
            e.ok()
                .filter(|e| e.file_type().is_file())
                .map(|e| e.into_path())
        })
        .collect()
}

pub fn all_rom_sources(root: &Path) -> HashMap<Part, PathBuf> {
    use rayon::prelude::*;

    subdir_files(root)
        .into_par_iter()
        .filter_map(|path| Part::from_path(&path).ok().map(|part| (part, path)))
        .collect()
}

pub fn get_rom_sources(root: &Path, required: HashSet<Part>) -> HashMap<Part, PathBuf> {
    use rayon::prelude::*;

    subdir_files(root)
        .into_par_iter()
        .filter_map(|path| {
            Part::from_path(&path)
                .ok()
                .filter(|part| required.contains(part))
                .map(|part| (part, path))
        })
        .collect()
}

pub fn copy(part: &Part, source: &Path, target: &Path) -> Result<(), std::io::Error> {
    use std::fs::{copy, create_dir_all, hard_link, remove_file};

    if !target.exists() {
        create_dir_all(target.parent().unwrap())?;
        hard_link(source, target).or_else(|_| copy(source, target).map(|_| ()))?;
        println!("{} -> {}", source.display(), target.display());
    } else if let Some(VerifyFailure::Bad(target)) = part.verify(target) {
        remove_file(target)?;
        hard_link(source, target).or_else(|_| copy(source, target).map(|_| ()))?;
        println!("{} -> {}", source.display(), target.display());
    }
    Ok(())
}

pub fn dry_run(part: &Part, source: &Path, target: &Path) -> Result<(), std::io::Error> {
    if !target.exists() {
        println!("{} -> {}", source.display(), target.display());
    } else if let Some(VerifyFailure::Bad(target)) = part.verify(target.to_path_buf()) {
        println!("{} -> {}", source.display(), target.display());
    }
    Ok(())
}
