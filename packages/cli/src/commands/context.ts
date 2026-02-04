/**
 * context command - Show context bundle for a page
 */

import chalk from 'chalk';
import { getContextBundle, getTemplateContextBundle } from '@wikitool/core';
import { withContext } from '../utils/context.js';
import { printError, printSection } from '../utils/format.js';
import { buildMeta, withMeta } from '../utils/meta.js';

export interface ContextOptions {
  json?: boolean;
  sections?: string;
  full?: boolean;
  template?: boolean;
  meta?: boolean;
}

export async function contextCommand(title: string, options: ContextOptions): Promise<void> {
  try {
    await withContext(async (ctx) => {
      const maxSections = options.sections ? parseInt(options.sections, 10) : undefined;
      const includeContent = options.full;
      const sectionLimit = Number.isFinite(maxSections) ? maxSections : undefined;

      if (options.template) {
        const templateBundle = getTemplateContextBundle(ctx.db, title, {
          includeContent,
          maxSections: sectionLimit,
        });

        if (options.json) {
          const output = options.meta === false
            ? templateBundle
            : withMeta(templateBundle, buildMeta(ctx));
          console.log(JSON.stringify(output, null, 2));
          return;
        }

        console.log(chalk.bold(`Template Context: ${templateBundle.templateName}`));
        console.log(chalk.dim(`Page: ${templateBundle.pageTitle}`));

        if (!templateBundle.page) {
          printError(`Template page not found: ${templateBundle.pageTitle}`);
          return;
        }

        printSection('Usage');
        console.log(`  Total calls: ${templateBundle.usage.totalCalls}`);
        console.log(`  Pages using: ${templateBundle.usage.totalPages}`);

        if (templateBundle.usage.namedParams.length > 0) {
          printSection('Named Parameters');
          for (const param of templateBundle.usage.namedParams.slice(0, 12)) {
            console.log(`  ${param.name}: ${param.usageCount}`);
          }
          if (templateBundle.usage.namedParams.length > 12) {
            console.log(chalk.dim(`  ... and ${templateBundle.usage.namedParams.length - 12} more`));
          }
        }

        if (templateBundle.usage.positionalParams.length > 0) {
          printSection('Positional Parameters');
          for (const param of templateBundle.usage.positionalParams.slice(0, 8)) {
            console.log(`  #${param.index}: ${param.usageCount}`);
          }
          if (templateBundle.usage.positionalParams.length > 8) {
            console.log(chalk.dim(`  ... and ${templateBundle.usage.positionalParams.length - 8} more`));
          }
        }

        if (templateBundle.schema.notes.length > 0) {
          printSection('Schema Notes');
          for (const note of templateBundle.schema.notes) {
            console.log(`  - ${note}`);
          }
        }

        return;
      }

      const bundle = getContextBundle(ctx.db, title, {
        includeContent,
        maxSections: sectionLimit,
      });

      if (!bundle) {
        printError(`Page not found: ${title}`);
        process.exit(1);
      }

      if (options.json) {
        const output = options.meta === false
          ? bundle
          : withMeta(bundle, buildMeta(ctx));
        console.log(JSON.stringify(output, null, 2));
        return;
      }

      console.log(chalk.bold(`Context: ${bundle.title}`));
      if (bundle.shortdesc) {
        console.log(chalk.dim(bundle.shortdesc));
      }

      printSection('Summary');
      console.log(`  Namespace: ${bundle.namespace}`);
      console.log(`  Type: ${bundle.pageType}`);
      console.log(`  Word count: ${bundle.wordCount ?? 'n/a'}`);
      console.log(`  Sections: ${bundle.sections.length}`);
      console.log(`  Templates: ${bundle.templates.length}`);
      console.log(`  Categories: ${bundle.categories.length}`);

      if (bundle.infobox.length > 0) {
        printSection('Infobox');
        for (const entry of bundle.infobox.slice(0, 15)) {
          console.log(`  ${entry.paramName}: ${entry.paramValue ?? ''}`);
        }
        if (bundle.infobox.length > 15) {
          console.log(chalk.dim(`  ... and ${bundle.infobox.length - 15} more`));
        }
      }

      printSection('Sections');
      for (const section of bundle.sections) {
        const label = section.heading ? `${section.heading} (L${section.level ?? 0})` : 'Lead';
        console.log(`  ${label}`);
      }

      if (bundle.templateMetadata) {
        printSection('Template Metadata');
        console.log(`  Source: ${bundle.templateMetadata.source}`);
        if (bundle.templateMetadata.description) {
          console.log(`  Description: ${bundle.templateMetadata.description}`);
        }
      }

      if (bundle.templateUsage) {
        printSection('Template Usage');
        console.log(`  Total calls: ${bundle.templateUsage.totalCalls}`);
        console.log(`  Pages using: ${bundle.templateUsage.totalPages}`);
        if (bundle.templateUsage.namedParams.length > 0) {
          for (const param of bundle.templateUsage.namedParams.slice(0, 10)) {
            console.log(`  ${param.name}: ${param.usageCount}`);
          }
          if (bundle.templateUsage.namedParams.length > 10) {
            console.log(chalk.dim(`  ... and ${bundle.templateUsage.namedParams.length - 10} more`));
          }
        }
      }

      if (bundle.moduleDependencies && bundle.moduleDependencies.length > 0) {
        printSection('Module Dependencies');
        for (const dep of bundle.moduleDependencies) {
          console.log(`  ${dep.depType}: ${dep.dependency}`);
        }
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}
