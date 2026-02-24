#!/usr/bin/env node

/**
 * Accessibility Testing Script
 * Runs axe-core accessibility tests on key pages
 */

const { chromium } = require('playwright');
const { AxePuppeteer } = require('@axe-core/puppeteer');

const pages = [
  { name: 'Home', url: 'http://localhost:3000' },
  { name: 'Dashboard', url: 'http://localhost:3000/dashboard' },
  { name: 'Groups', url: 'http://localhost:3000/groups' },
  { name: 'Profile', url: 'http://localhost:3000/profile' },
];

async function runAccessibilityTests() {
  console.log('🔍 Starting accessibility tests...\n');

  const browser = await chromium.launch();
  const page = await browser.newPage();

  let totalViolations = 0;
  const results = [];

  for (const testPage of pages) {
    console.log(`Testing: ${testPage.name} (${testPage.url})`);

    try {
      await page.goto(testPage.url, { waitUntil: 'networkidle' });

      const accessibilityResults = await page.evaluate(async () => {
        const axe = require('axe-core');
        return await axe.run();
      });

      const violations = accessibilityResults.violations;
      totalViolations += violations.length;

      results.push({
        page: testPage.name,
        url: testPage.url,
        violations: violations.length,
        passes: accessibilityResults.passes.length,
        details: violations,
      });

      if (violations.length === 0) {
        console.log(`✅ No violations found\n`);
      } else {
        console.log(`❌ Found ${violations.length} violations:`);
        violations.forEach((violation, index) => {
          console.log(`  ${index + 1}. ${violation.id}: ${violation.description}`);
          console.log(`     Impact: ${violation.impact}`);
          console.log(`     Affected elements: ${violation.nodes.length}`);
        });
        console.log('');
      }
    } catch (error) {
      console.error(`Error testing ${testPage.name}:`, error.message);
    }
  }

  await browser.close();

  // Summary
  console.log('\n📊 Test Summary:');
  console.log(`Total pages tested: ${pages.length}`);
  console.log(`Total violations: ${totalViolations}`);

  results.forEach((result) => {
    const status = result.violations === 0 ? '✅' : '❌';
    console.log(`${status} ${result.page}: ${result.violations} violations, ${result.passes} passes`);
  });

  // Exit with error if violations found
  if (totalViolations > 0) {
    console.log('\n❌ Accessibility tests failed');
    process.exit(1);
  } else {
    console.log('\n✅ All accessibility tests passed!');
    process.exit(0);
  }
}

runAccessibilityTests().catch((error) => {
  console.error('Fatal error:', error);
  process.exit(1);
});
