extern crate getopts;
use std::cmp;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write, Cursor};
use std::path::Path;
use getopts::{Options, Fail, Matches};
use image::{ImageFormat, io::Reader};
use lofty::{self, Tag, AudioTag, Picture, MimeType};

#[derive(PartialEq, Eq, Hash, Debug)]
enum Field {
    Track,
    Year,
    Disc,

    Title,
    Artist,
    Album,
    AlbumArtist,

    Image,
}

#[derive(Debug)]
enum Data {
    Str(String),
    Int(i32),
    File(String),
    StdIn,
}

#[derive(Debug)]
enum Command {
    Print(Field),
    Clear(Field),
    Set(Field, Data),
}


/// Returns on the failure of `Config::new` and `Config::exec`.
pub struct Error {
    /// An error code expected to be returned when the program ends. An error 
    /// code of...
    ///
    /// `0` means that an expected result occured, but the program must end
    /// now, (like in the case of a `--help` flag).
    ///
    /// `1` means there was an error with the arguments inputted.
    ///
    /// `2` means there was a file related error.
    ///
    /// `3` means something that a number wasn't parsed correctly.
    ///
    /// `4` means a field specified doesn't exist.
    ///
    /// `5` means a field tried to be cleared and set at the same time.
    ///
    /// `6` means that there were no "free parameters", aka filenames
    ///
    /// `7` means that there was an error when trying to edit the tags of a file
    pub error_code: i32,
    
    /// String expected to be printed right before the end of the program.
    pub error_str: String,  
}

impl Error {
    fn new(name: &str, opts: &Options, error_str: Option<&str>, error_code: i32) -> Error {
        let brief = format!("Usage: {} [options] <FILE(s)>", name);
        let usage = opts.usage(&brief);

        let error_str = error_str.unwrap_or(&usage).to_string();

        Error { error_code, error_str }
    }
}

/// Parses arguments and executes the main program
pub struct Config {
    files: Vec<String>,
    commands: Vec<Command>,
    opts: Options,
    name: String,
}

fn str_to_field(s: &str) -> Option<Field> {
    match s {
        "track" => Some(Field::Track),
        "year" => Some(Field::Year),
        "disc" => Some(Field::Disc),

        "title" => Some(Field::Title),
        "artist" => Some(Field::Artist),
        "album" => Some(Field::Album),
        "albumartist" => Some(Field::AlbumArtist),

        "image" => Some(Field::Image),

        _ => None 
    }
}

#[allow(dead_code)]
fn field_to_str(f: &Field) -> &str {
    match f {
        Field::Track => "track",
        Field::Year => "year",
        Field::Disc => "disc",

        Field::Title => "title",
        Field::Artist => "artist",
        Field::Album => "album",
        Field::AlbumArtist => "albumartist",

        Field::Image => "image",
    }
}

fn printout(tag: &dyn AudioTag) -> String {
    let mut result = String::new();
    result.push_str(&format!("Disc: {}\n", tag.disc_number().unwrap_or(0)));
    result.push_str(&format!("Track: {}\n", tag.track_number().unwrap_or(0)));
    result.push_str(&format!("Title: {}\n", tag.title().unwrap_or("")));
    result.push_str(&format!("Artist: {}\n", tag.artist_str().unwrap_or("")));
    result.push_str(&format!("Album: {}\n", tag.album_title().unwrap_or("")));
    result.push_str(&format!("Album Arist: {}\n", tag.album_artist_str().unwrap_or("")));
    result.push_str(&format!("Image: {}\n", match tag.album_cover() { Some(_) => "Present", None => "No image" }));
    result.push_str(&format!("Year: {}\n", tag.year().unwrap_or(0)));

    result
}

impl Config {
    /// Parses arguments and creates a Config struct
    pub fn new(args: &[String], name: &str) -> Result<Config, Error> { 
       let mut opts = Options::new();

       // Flags
       opts.optflag("h", "help", "Print this help text");

       // Options
       opts.optmulti("", "clear", "Clear out a field", "FIELD");

       // Field Options
       opts.optflagopt("", "track", "The track number", "NUM");
       opts.optflagopt("", "year", "The year the track released", "NUM");
       opts.optflagopt("", "disc", "The disc this track is on", "NUM");

       opts.optflagopt("", "title", "The song name", "STRING");
       opts.optflagopt("", "artist", "The song's artist", "STRING");
       opts.optflagopt("", "album", "The song's album", "STRING");
       opts.optflagopt("", "albumartist", "The album artist", "STRING");
       opts.optflagopt("", "comment", "A description/comment about the song", "STRING");

       opts.optflagopt("", "image", "The album artwork/photo that goes along with the song. `-` for stdin, `./-` for a file literally named `-`.", "FILE");

       let matches: Matches;

       match opts.parse(args) {
            Ok(m) => matches = m,
            Err(f) => {
                let err_str = match f {
                    Fail::ArgumentMissing(o) => format!("Argument for option '{}' missing", o),
                    Fail::UnrecognizedOption(o) => format!("Unknown option '{}'", o),
                    Fail::OptionMissing(o) => format!("Option '{}' missing", o),
                    Fail::OptionDuplicated(o) => format!("Option '{}' used more than once", o),
                    Fail::UnexpectedArgument(o) => format!("Unexpected argument for '{}'", o)
                };
                return Err(Error::new(name, &opts, Some(&err_str), 1));
            }
       }

       // Make sure some files are specified
       if matches.free.len() == 0 {
           let error_str = "There were no files specified.";
           return Err(Error::new(name, &opts, Some(error_str), 6));
       }

       // Verify each file does exist
       for f in &matches.free {
           if !(Path::new(&f).is_file()) {
               let err_str = format!("File {} does not exist, is a broken symlink, or we may not have valid permissions", &f);
               return Err(Error::new(name, &opts, Some(&err_str), 2));
           }
       }

       // Flags
       if matches.opt_present("help") {
            return Err(Error::new(name, &opts, None, 0));
       }
       
       // Fields
       let mut commands: Vec<Command> = Vec::new();

       // Integer Fields

       if matches.opt_present("track") {
           if let Some(s) = matches.opt_str("track") {
                let val;
                match i32::from_str_radix(s.trim(),10) {
                    Ok(i) => val = i,
                    Err(_) => { 
                        let err_str = "'track', 'year', and 'disc' feeds need to be integers. (Error on 'track' field)";
                        return Err(Error::new(name, &opts, Some(err_str), 3));
                    }
                }   
                commands.push(Command::Set(Field::Track, Data::Int(val)));
           } else {
               commands.push(Command::Print(Field::Track));
           }
       }

       if matches.opt_present("year") {
           if let Some(s) = matches.opt_str("year") {
                let val;
                match i32::from_str_radix(s.trim(),10) {
                    Ok(i) => val = i,
                    Err(_) => { 
                        let err_str = "'track', 'year', and 'disc' feeds need to be integers. (Error on 'year' field)";
                        return Err(Error::new(name, &opts, Some(err_str), 3));
                    }
                }   
                commands.push(Command::Set(Field::Year, Data::Int(val)));
           } else {
               commands.push(Command::Print(Field::Year));
           }
       }

       if matches.opt_present("disc") {
           if let Some(s) = matches.opt_str("disc") {
                let val;
                match i32::from_str_radix(s.trim(),10) {
                    Ok(i) => val = i,
                    Err(_) => { 
                        let err_str = "'track', 'year', and 'disc' feeds need to be integers. (Error on 'disc' field)";
                        return Err(Error::new(name, &opts, Some(err_str), 3));
                    }
                }   
                commands.push(Command::Set(Field::Disc, Data::Int(val)));
           } else {
               commands.push(Command::Print(Field::Disc));
           }
       }

       // String Fields

       if matches.opt_present("title") {
           if let Some(s) = matches.opt_str("title") {
               commands.push(Command::Set(Field::Title, Data::Str(s)));
           } else {
               commands.push(Command::Print(Field::Title));
           }
       }

       if matches.opt_present("artist") {
           if let Some(s) = matches.opt_str("artist") {
               commands.push(Command::Set(Field::Artist, Data::Str(s)));
           } else {
               commands.push(Command::Print(Field::Artist));
           }

       }

       if matches.opt_present("album") {
           if let Some(s) = matches.opt_str("album") {
               commands.push(Command::Set(Field::Album, Data::Str(s)));
           } else {
               commands.push(Command::Print(Field::Album));
           }

       }

       if matches.opt_present("albumartist") {
           if let Some(s) = matches.opt_str("albumartist") {
               commands.push(Command::Set(Field::AlbumArtist, Data::Str(s)));
           } else {
               commands.push(Command::Print(Field::AlbumArtist));
           }

       }

       // File Fields
       
       if matches.opt_present("image") {
           if let Some(s) = matches.opt_str("image") {
                if s != "-" { // If we shouldn't read from stdin
                    if !(Path::new(&s).is_file()) {
                        let err_str = format!("File {} does not exist, is a broken symlink, or we may not have valid permissions", &s);
                        return Err(Error::new(name, &opts, Some(&err_str), 2));
                    } else {
                        commands.push(Command::Set(Field::Image, Data::File(s)));
                    }
                } else {
                    commands.push(Command::Set(Field::Image, Data::StdIn));
                }
           } else {
               commands.push(Command::Print(Field::Image));
           }

       }

       // Clear option

       let mut used: HashSet<&Field> = HashSet::new();
       let mut clear_commands: Vec<Command> = Vec::new();

       for c in &commands { // Find all of the fields for the set commands
           match c {
               Command::Set(f, _) => { used.insert(&f); }
               Command::Print(f) => { used.insert(&f); }
               Command::Clear(_) => { /* no-op */ },
           }
       }

       for s in matches.opt_strs("clear") { // For every clear command...
           if let Some(f) = str_to_field(&s) {
               if !(used.contains(&f)) { // If the field isn't in used in a set command
                   clear_commands.push(Command::Clear(f)); // Then add a clear command
               } else { // If the field is in the set command, error.
                   let err_str = format!("Cannot clear and set/print field '{}' at the same time", &s);
                   return Err(Error::new(name, &opts, Some(&err_str), 5));
               }
           } else { // If the clear command didn't contain a valid field, error.
               let err_str = format!("Cannot clear '{}' field because it does not exist!", &s);
               return Err(Error::new(name, &opts, Some(&err_str), 4));
           }
       }

       for c in clear_commands {
           commands.push(c);
       }

       Ok(Config {
           files: matches.free,
           commands: commands,
           opts: opts,
           name: name.to_string(),
       })
    }

    /// The main part of the program that does the metadata modifications
    pub fn exec(self) -> Result<(), Error> {
        for f in &self.files {
            let mut tag = match Tag::new().read_from_path_signature(f) {
                Ok(t) => t,
                Err(_) => { 
                    let err_str = format!("Failure to open `{}` for editing", f);
                    return Err(Error::new(&self.name, &self.opts, Some(&err_str), 7));
                }
            };
            
            if self.commands.is_empty() {
                println!("{}", printout(&(*tag)));
            } else {
                let mut need_to_write = false;
                let mut did_print = false;
    
                for c in &self.commands {
                    match c {
                        Command::Set(f, d) => {
                            need_to_write = true;

                            match f {
                                // Int Fields
                                Field::Disc => {
                                    if let Data::Int(i) = d {
                                        tag.set_disc_number(cmp::max(*i, 0) as u32);
                                    }
                                    else { panic!("d isn't a int (disc)"); }
                                }
                                Field::Track => {
                                    if let Data::Int(i) = d {
                                        tag.set_track_number(cmp::max(*i, 0) as u32);
                                    }
                                    else { panic!("d isn't a int (track)"); }
                                }
                                Field::Year => {
                                    if let Data::Int(i) = d {
                                        tag.set_year(*i);
                                    }
                                    else { panic!("d isn't a int (year)"); }
                                }

                                // Title Fields
                                Field::Title => {
                                    if let Data::Str(s) = d {
                                        tag.set_title(s);
                                    }
                                    else { panic!("d isn't a string (title)"); }
                                }
                                Field::Artist => {
                                    if let Data::Str(s) = d {
                                        tag.set_artist(s);
                                    }
                                    else { panic!("d isn't a string (artist)"); }
                                }
                                Field::Album => {
                                    if let Data::Str(s) = d {
                                        tag.set_album_title(s);
                                    }
                                    else { panic!("d isn't a string (album)"); }
                                }
                                Field::AlbumArtist => {
                                    if let Data::Str(s) = d {
                                        tag.set_album_artist(s);
                                    }
                                    else { panic!("d isn't a string (albumartist)"); }
                                }

                                // File Fields
                                Field::Image => {
                                    let mut buf: Vec<u8> = Vec::new();
                                    
                                    if let Data::File(s) = d {
                                        let mut f = match File::open(s) {
                                            Ok(f) => f,
                                            Err(_) => {
                                                let error_str = "Issue when opening image file.";
                                                return Err(Error::new(&self.name, &self.opts, Some(error_str), 2));
                                            }
                                        };

                                        if let Err(_) = f.read_to_end(&mut buf) {
                                            let error_str = "Issue when reading image file.";
                                            return Err(Error::new(&self.name, &self.opts, Some(error_str), 2));
                                        }
                                    }

                                    else if let Data::StdIn = d {
                                        let mut stdin = io::stdin();

                                        if let Err(_) = stdin.read_to_end(&mut buf) {
                                            let error_str = "Issue when reading stdin.";
                                            return Err(Error::new(&self.name, &self.opts, Some(error_str), 2));
                                        }
                                    }

                                    else { panic!("d isn't a file or stdin (image)"); }

                                    let reader = Reader::new(Cursor::new(&buf))
                                        .with_guessed_format().expect("'cursor io never fails'");
                                    let mimetype = match reader.format() {
                                        Some(ImageFormat::Png) => MimeType::Png,
                                        Some(ImageFormat::Jpeg) => MimeType::Jpeg,
                                        Some(ImageFormat::Tiff) => MimeType::Tiff,
                                        Some(ImageFormat::Bmp) => MimeType::Bmp,
                                        Some(ImageFormat::Gif) => MimeType::Gif,
                                        _ => {
                                            let error_str = "Unsupported image format (Supported: Png, Jpeg, Tiff, Bmp, Gif)";
                                            return Err(Error::new(&self.name, &self.opts, Some(error_str), 2));
                                        }
                                    };

                                    let picture = Picture::new(&buf, mimetype);
                                    tag.set_album_cover(picture);
                                }
                            }
                        }
                        Command::Clear(f) => {
                            need_to_write = true;

                            match f {
                                // Int Fields
                                Field::Disc => tag.remove_disc_number(),
                                Field::Track => tag.remove_track_number(),
                                Field::Year => tag.remove_year(),

                                // Str Fields
                                Field::Title => tag.remove_title(),
                                Field::Artist => tag.remove_artist(),
                                Field::Album => tag.remove_album_title(),
                                Field::AlbumArtist => tag.remove_album_artists(),

                                // File Fields
                                Field::Image => tag.remove_album_cover(),
                            }
                        }
                        Command::Print(f) => {
                            did_print = true;

                            match f {
                                // Int Fields
                                Field::Disc => println!("{}", tag.disc_number().unwrap_or(0)),
                                Field::Track => println!("{}", tag.track_number().unwrap_or(0)),
                                Field::Year => println!("{}", tag.year().unwrap_or(0)),

                                // Str Fields
                                Field::Title => println!("{}", tag.title().unwrap_or("")),
                                Field::Artist => println!("{}", tag.artist_str().unwrap_or("")),
                                Field::Album => println!("{}", tag.album_title().unwrap_or("")),
                                Field::AlbumArtist => println!("{}", tag.album_artist_str().unwrap_or("")),

                                // File Fields
                                Field::Image => {
                                    if let Some(p) = tag.album_cover() {
                                        if let Err(_) = io::stdout().write_all(p.data) {
                                            // This error message probably won't even make it to
                                            // the user, lol.
                                            let error_str = "Error when trying to print image to stdout";
                                            return Err(Error::new(&self.name, &self.opts, Some(error_str), 2));
                                        }
                                        println!(); // Write a newline separator
                                    } else { println!(); }
                                }
                            }
                        }
                    }
                }
    
                if need_to_write {
                    if let Err(_) = tag.write_to_path(f) {
                        let error_str = format!("Failed to write new tags to {}", f);
                        return Err(Error::new(&self.name, &self.opts, Some(&error_str), 2));
                    }
                }

                if !did_print {
                    println!("{}", printout(&(*tag)));
                }
            }
        }

        Ok(())
    }
}
