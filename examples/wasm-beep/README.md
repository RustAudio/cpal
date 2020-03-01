## How to install

```sh
npm install
```

## How to run in debug mode

```sh
# Builds the project and opens it in a new browser tab. Auto-reloads when the project changes.
npm start
```

## How to build in release mode

```sh
# Builds the project and places it into the `dist` folder.
npm run build
```

## What does each file do?

* `Cargo.toml` contains the standard Rust metadata. You put your Rust dependencies in here. You must change this file with your details (name, description, version, authors, categories)

* `package.json` contains the standard npm metadata. You put your JavaScript dependencies in here. You must change this file with your details (author, name, version)

* `webpack.config.js` contains the Webpack configuration. You shouldn't need to change this, unless you have very special needs.

* The `src` folder contains your Rust code.
