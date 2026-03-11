"""
Bulk migrate react-router-dom imports to Next.js equivalents in all dashboard src files.
Also adds 'use client' to any file using React hooks.
"""
import os
import re

SRC = "/Users/shashisharma/code/self/flowbase/dashboard/src"

# Walk all .tsx/.ts files
for root, dirs, files in os.walk(SRC):
    # Skip the app directory (already Next.js)
    dirs[:] = [d for d in dirs if d != 'app']
    for fname in files:
        if not fname.endswith(('.tsx', '.ts')):
            continue
        path = os.path.join(root, fname)
        with open(path) as f:
            content = f.read()

        if 'react-router-dom' not in content:
            continue

        original = content

        # 1. Replace combined imports
        # useNavigate + useParams + Link
        content = re.sub(
            r"import \{ useNavigate, useParams, Link \} from 'react-router-dom'",
            "import { useParams, useRouter } from 'next/navigation'\nimport Link from 'next/link'",
            content
        )
        # useNavigate + useParams (various orderings)
        content = re.sub(
            r"import \{ useNavigate, useParams \} from 'react-router-dom'",
            "import { useParams, useRouter } from 'next/navigation'",
            content
        )
        content = re.sub(
            r"import \{ useParams, useNavigate \} from 'react-router-dom'",
            "import { useParams, useRouter } from 'next/navigation'",
            content
        )
        # useParams + Link
        content = re.sub(
            r"import \{ useParams, Link \} from 'react-router-dom'",
            "import { useParams } from 'next/navigation'\nimport Link from 'next/link'",
            content
        )
        # Single imports
        content = re.sub(
            r"import \{ useParams \} from 'react-router-dom'",
            "import { useParams } from 'next/navigation'",
            content
        )
        content = re.sub(
            r"import \{ useNavigate \} from 'react-router-dom'",
            "import { useRouter } from 'next/navigation'",
            content
        )
        content = re.sub(
            r"import \{ Link \} from 'react-router-dom'",
            "import Link from 'next/link'",
            content
        )

        # 2. Replace hook usage
        content = content.replace(
            "const navigate = useNavigate()",
            "const router = useRouter()"
        )
        # navigate('/path') → router.push('/path')  (handles single and template literals)
        content = re.sub(r'\bnavigate\((`[^`]+`|\'[^\']+\')\)', lambda m: f'router.push({m.group(1)})', content)

        # 3. Add 'use client' if not already present and file uses hooks/client APIs
        client_indicators = ['useParams', 'useRouter', 'useState', 'useEffect', 'useRef', 'useCallback', 'useMemo', 'Link']
        needs_client = any(ind in content for ind in client_indicators)
        if needs_client and not content.startswith("'use client'"):
            content = "'use client'\n\n" + content

        if content != original:
            with open(path, 'w') as f:
                f.write(content)
            rel = os.path.relpath(path, SRC)
            print(f"  migrated {rel}")

print("Done")
