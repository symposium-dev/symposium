# Mynah UI
This package is the whole UI of AWS Codewhisperer Chat extension UI for Web, VSCode and Intellij IDEs written in typescript without any framework or third-party UI library dependency. Purpose of the separated UI is to handle the interactions and look & feel of the UI from one single source. 

## How to release
### Production 
You need to create a new release from your desired branch with a tag which should follow the naming `v*.*` to release a production version to npm. The tag you're creating shouldn't be existed before. 

### Beta releases
If you need to release a beta version first you need to specify the version name inside `package.json` which should follow the versioning `2.0.0-beta.1`
After that you need to create a new release from your desired branch with a tag which should follow the naming `beta*.*` to release a beta version to npm. The tag you're creating shouldn't be existed before. 

``` console
please see publish.yml and beta.yml for releasing details.
```


## Security

See [CONTRIBUTING](CONTRIBUTING.md#security-issue-notifications) for more information.

## License

This project is licensed under the Apache-2.0 License.

