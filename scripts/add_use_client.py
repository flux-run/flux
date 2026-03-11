"""Add 'use client' to any page/component that uses client-side React APIs."""
import os, re

DIRS = [
    "/Users/shashisharma/code/self/flowbase/dashboard/src/pages",
    "/Users/shashisharma/code/self/flowbase/dashboard/src/components",
    "/Users/shashisharma/code/self/flowbase/dashboard/src/hooks",
]

CLIENT_INDICATORS = [
    'useState', 'useEffect', 'useRef', 'useCallback', 'useMemo',
    'useReducer', 'useContext', 'useId', 'useTransition', 'useDeferredValue',
    'useQuery', 'useMutation', 'useQueryClient',   # tanstack
    'useStore',                                     # zustand
    'useRouter', 'useParams', 'usePathname',        # next/navigation
    'useAuth',                                      # custom
    'onClick', 'onChange', 'onSubmit',              # event handlers in JSX
]

for base in DIRS:
    for root, dirs, files in os.walk(base):
        dirs[:] = [d for d in dirs]
        for fname in files:
            if not fname.endswith(('.tsx', '.ts')):
                continue
            path = os.path.join(root, fname)
            with open(path) as f:
                content = f.read()

            if content.startswith("'use client'"):
                continue

            if any(ind in content for ind in CLIENT_INDICATORS):
                content = "'use client'\n\n" + content
                with open(path, 'w') as f:
                    f.write(content)
                print(f"  added 'use client' to {os.path.relpath(path)}")

print("Done")
