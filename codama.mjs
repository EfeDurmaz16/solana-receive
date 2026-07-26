export default {
  // @codama/nodes is overridden to 1.8.0 in package.json: 1.10 omits `fields` on empty
  // structs and @codama/renderers-js@2.3 crashes when rendering InstructionExtra.
  idl: "idl/token-2022-receive.json",
  before: [],
  scripts: {
    js: {
      from: "@codama/renderers-js",
      args: [
        "clients/js",
        {
          kitImportStrategy: "rootOnly",
          syncPackageJson: false,
          prettierOptions: {
            arrowParens: "avoid",
            printWidth: 100,
            singleQuote: true,
            tabWidth: 2,
            trailingComma: "all",
          },
        },
      ],
    },
  },
};
