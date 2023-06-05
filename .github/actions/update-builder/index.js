require('../../bootstrap').invokeWith(({ getInput }) => {
    return [
        'update-builder',

        '--buildpack-id',
        getInput('buildpack-id', { required: true }),

        '--buildpack-version',
        getInput('buildpack-version', { required: true }),

        '--buildpack-uri',
        getInput('buildpack-uri', { required: true }),

        '--builders',
        getInput('builders', { required: true }),
    ]
})