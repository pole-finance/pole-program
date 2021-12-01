module.exports = {
    "env": {
        "browser": true,
        "es2021": true
    },
    "extends": [
        "eslint:recommended",
        "plugin:react/recommended",
        "plugin:@typescript-eslint/recommended"
    ],
    "parser": "@typescript-eslint/parser",
    "parserOptions": {
        "ecmaFeatures": {
            "jsx": true
        },
        "ecmaVersion": 13,
        "sourceType": "module"
    },
    "plugins": [
        "react",
        "react-hooks",
        "@typescript-eslint"
    ],
    "rules": {
        "react/react-in-jsx-scope": "off",// React17后 不需要引入 React
        "@typescript-eslint/explicit-module-boundary-types": "off", // TS可以隐式判断
        "@typescript-eslint/no-var-requires": "off"
    }
};
