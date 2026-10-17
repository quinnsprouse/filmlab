export type CustomProperty = `--${string}`;

declare module "react" {
  interface CSSProperties {
    [property: CustomProperty]: string | undefined;
  }
}
