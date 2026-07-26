export default {
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
