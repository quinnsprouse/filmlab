export default {
  extends: ["@commitlint/config-conventional"],
  rules: {
    "body-max-line-length": [0, "always"],
    "footer-max-line-length": [0, "always"],
    "no-co-authored-by": [2, "always"],
  },
  plugins: [
    {
      rules: {
        "no-co-authored-by": ({ raw }: { raw: string }) => {
          const has = /Co-Authored-By:/i.test(raw);
          return [!has, "Commit must not contain Co-Authored-By trailers"];
        },
      },
    },
  ],
};
