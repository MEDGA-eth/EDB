import { GlobalRegistrator } from '@happy-dom/global-registrator';
GlobalRegistrator.register();

import * as matchers from '@testing-library/jest-dom/matchers';
import { expect } from 'bun:test';
expect.extend(matchers as never);
