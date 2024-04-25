# jimaku (字幕)

jimaku is a simple site dedicated to hosting Japanese subtitles of anime or other Japanese content. It's the spiritual successor of [kitsunekko](https://kitsunekko.net).


# Install

Right now, Rust v1.74 or higher is required. To install just run `cargo build`.

In order to actually run the server the `static` directory needs to be next to the executable. Maybe in the future there'll be a way to automatically move it.

# Configuration

Configuration is done using a JSON file. The location of the configuration file depends on the operating system:

- Linux: `$XDG_CONFIG_HOME/jimaku/config.json` or `$HOME/.config/jimaku/config.json`
- macOS: `$HOME/Library/Application Support/jimaku/config.json`
- Windows: `%AppData%/jimaku/config.json`

The documentation for the actual configuration options is documented in the [source code](src/config.rs).

## Create the admin account

To manage the site you need an admin account.
To create one interactively,
run `cargo run admin`.

## Get help

To print a help page, run `cargo run -- --help`.

## Data and Logs

The server also contains a database and some logs which are written to different directories depending on the operating system as well:

For data it is as follows:

- Linux: `$XDG_DATA_HOME/jimaku` or `$HOME/.local/share/jimaku`
- macOS: `$HOME/Library/Application Support/jimaku`
- Windows: `%AppData%/jimaku`

For logs it is as follows:

- Linux: `$XDG_STATE_HOME/jimaku` or `$HOME/.local/state/jimaku`
- macOS: `./logs`
- Windows: `./logs`

The data directory contains both a database and a secluded managed "trash" directory.

# Fixtures

Since this site is made to be a replacement for [kitsunekko](https://kitsunekko.net), it has support for scraping and then loading the data to this server. In order to do this initially, a bootstrapping phase is necessary where it downloads all the files necessary and then generates a `fixture.json` file.

This `fixture.json` file essentially has all the data that was scraped in a format that the program can understand when loaded using the `fixtures` subcommand. This flow allows you to edit the data in a way without committing it to the database yet.

## Long Term Scraping

As a temporary migration period, this program supports periodically scraping from kitsunekko to get the newest data uploaded in a similar manner to the `fixtures` and `scrape` subcommands. Currently this is set to every hour.

# License

AGPL-v3.
