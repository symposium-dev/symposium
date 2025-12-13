declare module '*.svg' {
  const content: string;
  export default content;
}
declare module '*.scss';
declare const require: {
  context: (directory: string, useSubdirectories?: boolean, regExp?: RegExp) => any;
};
