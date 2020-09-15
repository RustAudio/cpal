const path = require("path");
const CopyPlugin = require("copy-webpack-plugin");
const WasmPackPlugin = require("@wasm-tool/wasm-pack-plugin");
const HtmlWebpackPlugin = require("html-webpack-plugin");

const dist = path.resolve(__dirname, "dist");

module.exports = {
  mode: "production",
  entry: {
    index: "./index.js"
  },
  output: {
    path: dist,
    filename: "[name].js"
  },
  devServer: {
    contentBase: dist,
  },
  plugins: [
    new HtmlWebpackPlugin({
      template: 'index.html'
    }),
    new WasmPackPlugin({
      crateDirectory: __dirname,
      extraArgs: "--out-name index"
    }),
  ]
};
