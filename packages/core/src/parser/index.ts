/**
 * Parser module exports
 */

export {
  extractLinks,
  extractTemplates,
  parseLink,
  parseContent,
  type ParsedLink,
  type ParsedContent,
} from './links.js';

export { calculateWordCount } from './wordcount.js';

export {
  extractMetadata,
  extractShortdesc,
  extractDisplayTitle,
  hasShortdesc,
  hasDisplayTitle,
  type PageMetadata,
} from './metadata.js';

export {
  parseSections,
  parseTemplateCalls,
  parseParserFunctions,
  parseTemplateData,
  parseModuleDependencies,
  type ParsedSection,
  type TemplateCall,
  type ParserFunctionCall,
  type TemplateParam,
  type TemplateMetadata,
  type ModuleDependency,
} from './context.js';

export {
  parseCargo,
  parseCargoColumnType,
  type CargoDeclare,
  type CargoStore,
  type CargoQuery,
  type CargoColumn,
  type CargoFieldType,
  type CargoConstruct,
} from './cargo.js';
