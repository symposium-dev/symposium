'use strict';

const path = require('path');
const HtmlWebpackPlugin = require('html-webpack-plugin');

/**@type {import('webpack').Configuration}*/
const config = {
    target: 'web',
    entry: './src/main.ts',
    output: {
        path: path.resolve(__dirname, 'dist'),
        filename: 'main.js',
        library: 'mynahWeb',
        libraryTarget: 'var',
        devtoolModuleFilenameTemplate: '../[resource-path]',
    },
    plugins: [
        new HtmlWebpackPlugin({
            template: 'src/index.html',
        }),
    ],
    devtool: 'source-map',
    resolve: {
        extensions: ['.ts', '.js'],
    },
    experiments: { asyncWebAssembly: true },
    module: {
        rules: [
            { test: /\.md$/, use: ['raw-loader'] },
            {
                test: /\.scss$/,
                use: [
                    'style-loader',
                    {
                        loader: 'css-loader',
                        options: {
                            importLoaders: 1,
                            modules: {
                                mode: 'icss', // Enable ICSS (Interoperable CSS)
                            },
                        },
                    },
                    'sass-loader',
                ],
            },
            {
                test: /\.ts$/,
                exclude: /node_modules/,
                use: [
                    {
                        loader: 'ts-loader',
                    },
                ],
            },
        ],
    },
};
module.exports = config;
