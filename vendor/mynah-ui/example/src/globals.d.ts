declare module "*.md";
declare interface Window { mynahUI: any; testInsertCode: string }
declare module "escape-html" {
    function escapeHTML(a:string): string;
    export = escapeHTML;
}