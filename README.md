# jimaku (字幕)

jimaku is a simple site dedicated to hosting Japanese subtitles of anime or other Japanese content. It's the spiritual successor of [kitsunekko](https://kitsunekko.net).


# Install

Right now, Rust v1.74 or higher is required. To install just run `cargo build`.

In order to actually run the server the `static` directory needs to be next to the executable. Maybe in the future there'll be a way to automatically move it.

# Configuration

To configure the server, edit `~/.config/jimaku/config.json`.

# Fixtures

Since this site is made to be a replacement for [kitsunekko](https://kitsunekko.net), it has support for scraping and then loading the data to this server. In order to do this initially, a bootstrapping phase is necessary where it downloads all the files necessary and then generates a `fixture.json` file.

This `fixture.json` file essentially has all the data that was scraped in a format that the program can understand when loaded using the `fixtures` subcommand. This flow allows you to edit the data in a way without committing it to the database yet.

## Long Term Scraping

As a temporary migration period, this program supports periodically scraping from kitsunekko to get the newest data uploaded in a similar manner to the `fixtures` and `scrape` subcommands. Currently this is set to every hour.

# Local data

The following directories are created when you run the server.

* Config `~/.config/jimaku`,
* Databases `~/.local/share/jimaku`,
* Logs `~/.local/state/jimaku`.

# License

AGPL-v3.
